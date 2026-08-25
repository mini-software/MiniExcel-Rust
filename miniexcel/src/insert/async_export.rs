use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures_core::Stream;
use futures_util::{FutureExt, StreamExt, pin_mut, select_biased};

use super::atomic::{AtomicCommitStage, export_to_path_with_hook};
use super::donor::save_dynamic_iter_to_writer;
use crate::{CancellationToken, DynamicRow, Error, Result, WriteOptions};

const ROW_CHANNEL_CAPACITY: usize = 16;

enum RowMessage {
    Row(Result<DynamicRow>),
    End,
}

enum WorkerStatus {
    Ready,
    Finished(Result<usize>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AsyncExportStage {
    BeforePreflight,
    ReadyForRows,
    Writing,
    Validation,
    BeforeCommit,
    Finished,
}

struct FutureCancellationGuard {
    cancellation: CancellationToken,
}

impl Drop for FutureCancellationGuard {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

pub(crate) async fn save_with_schema_async<S>(
    path: PathBuf,
    schema: Vec<String>,
    rows: S,
    options: WriteOptions,
    cancellation: CancellationToken,
) -> Result<usize>
where
    S: Stream<Item = Result<DynamicRow>>,
{
    save_with_schema_async_with_hook(path, schema, rows, options, cancellation, Arc::new(|_| {}))
        .await
}

async fn save_with_schema_async_with_hook<S>(
    path: PathBuf,
    schema: Vec<String>,
    rows: S,
    options: WriteOptions,
    cancellation: CancellationToken,
    phase_hook: Arc<dyn Fn(AsyncExportStage) + Send + Sync>,
) -> Result<usize>
where
    S: Stream<Item = Result<DynamicRow>>,
{
    if cancellation.is_cancelled() {
        return Err(Error::cancelled());
    }
    let operation_cancellation = CancellationToken::new();
    let _guard = FutureCancellationGuard { cancellation: operation_cancellation.clone() };
    let (row_sender, row_receiver) = async_channel::bounded(ROW_CHANNEL_CAPACITY);
    let (status_sender, status_receiver) = async_channel::bounded(2);
    let worker_cancellation = cancellation.clone();
    let worker_operation_cancellation = operation_cancellation.clone();
    let worker_hook = Arc::clone(&phase_hook);

    std::thread::Builder::new().name("miniexcel-async-export".to_owned()).spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_worker(
                &path,
                &schema,
                &options,
                row_receiver,
                &worker_cancellation,
                &worker_operation_cancellation,
                &status_sender,
                &worker_hook,
            )
        }))
        .unwrap_or_else(|_| Err(Error::insert_package("async export worker panicked")));
        let _ = status_sender.send_blocking(WorkerStatus::Finished(result));
        worker_hook(AsyncExportStage::Finished);
    })?;

    let status = match wait_for_status(&status_receiver, &cancellation).await {
        Ok(status) => status,
        Err(error) if error.is_cancelled() => {
            operation_cancellation.cancel();
            drop(row_sender);
            return wait_for_finished(&status_receiver).await;
        }
        Err(error) => return Err(error),
    };
    if let WorkerStatus::Finished(result) = status {
        return result;
    }

    pin_mut!(rows);
    loop {
        let next_row = rows.next().fuse();
        let next_status = status_receiver.recv().fuse();
        let cancelled = cancellation.cancelled().fuse();
        pin_mut!(next_row, next_status, cancelled);
        select_biased! {
            _ = cancelled => {
                operation_cancellation.cancel();
                drop(row_sender);
                return wait_for_finished(&status_receiver).await;
            }
            worker_status = next_status => {
                let worker_status = worker_status.map_err(|_| Error::insert_package("async export worker stopped without a result"))?;
                if let WorkerStatus::Finished(result) = worker_status {
                    return result;
                }
            }
            row = next_row => {
                let (message, finished) = match row {
                    Some(Err(error)) => (RowMessage::Row(Err(error)), true),
                    Some(row) => (RowMessage::Row(row), false),
                    None => (RowMessage::End, true),
                };
                let send = row_sender.send(message).fuse();
                let cancelled = cancellation.cancelled().fuse();
                pin_mut!(send, cancelled);
                select_biased! {
                    _ = cancelled => {
                        operation_cancellation.cancel();
                        drop(row_sender);
                        return wait_for_finished(&status_receiver).await;
                    }
                    result = send => {
                        if result.is_err() {
                            drop(row_sender);
                            return wait_for_finished(&status_receiver).await;
                        }
                    }
                }
                if finished {
                    drop(row_sender);
                    return wait_for_finished_or_cancel(
                        &status_receiver,
                        &cancellation,
                        &operation_cancellation,
                    ).await;
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_worker(
    path: &Path,
    schema: &[String],
    options: &WriteOptions,
    row_receiver: async_channel::Receiver<RowMessage>,
    cancellation: &CancellationToken,
    operation_cancellation: &CancellationToken,
    status_sender: &async_channel::Sender<WorkerStatus>,
    phase_hook: &Arc<dyn Fn(AsyncExportStage) + Send + Sync>,
) -> Result<usize> {
    phase_hook(AsyncExportStage::BeforePreflight);
    check_cancelled(cancellation, operation_cancellation)?;
    let mut ready_sent = false;
    export_to_path_with_hook(
        path,
        options.sheet_name(),
        options.overwrite_file(),
        |writer| {
            phase_hook(AsyncExportStage::Writing);
            let mut ended = false;
            let rows = std::iter::from_fn(|| {
                if ended {
                    return None;
                }
                if let Err(error) = check_cancelled(cancellation, operation_cancellation) {
                    ended = true;
                    return Some(Err(error));
                }
                match row_receiver.recv_blocking() {
                    Ok(RowMessage::Row(row)) => Some(row),
                    Ok(RowMessage::End) => {
                        ended = true;
                        None
                    }
                    Err(_) => {
                        ended = true;
                        Some(Err(Error::cancelled()))
                    }
                }
            });
            save_dynamic_iter_to_writer(writer, schema, rows, options)
        },
        |stage| {
            let phase = match stage {
                AtomicCommitStage::Preflight => AsyncExportStage::BeforePreflight,
                AtomicCommitStage::RowGeneration => {
                    if !ready_sent {
                        ready_sent = true;
                        phase_hook(AsyncExportStage::ReadyForRows);
                        check_cancelled(cancellation, operation_cancellation)?;
                        status_sender
                            .send_blocking(WorkerStatus::Ready)
                            .map_err(|_| Error::cancelled())?;
                    }
                    return check_cancelled(cancellation, operation_cancellation);
                }
                AtomicCommitStage::ZipCopy | AtomicCommitStage::ZipFinish => {
                    AsyncExportStage::Writing
                }
                AtomicCommitStage::Validation => AsyncExportStage::Validation,
                AtomicCommitStage::Commit => AsyncExportStage::BeforeCommit,
            };
            phase_hook(phase);
            check_cancelled(cancellation, operation_cancellation)
        },
    )
}

fn check_cancelled(
    cancellation: &CancellationToken,
    operation_cancellation: &CancellationToken,
) -> Result<()> {
    if cancellation.is_cancelled() || operation_cancellation.is_cancelled() {
        Err(Error::cancelled())
    } else {
        Ok(())
    }
}

async fn wait_for_status(
    status_receiver: &async_channel::Receiver<WorkerStatus>,
    cancellation: &CancellationToken,
) -> Result<WorkerStatus> {
    let status = status_receiver.recv().fuse();
    let cancelled = cancellation.cancelled().fuse();
    pin_mut!(status, cancelled);
    select_biased! {
        _ = cancelled => Err(Error::cancelled()),
        status = status => status.map_err(|_| Error::insert_package("async export worker stopped without a result")),
    }
}

async fn wait_for_finished(
    status_receiver: &async_channel::Receiver<WorkerStatus>,
) -> Result<usize> {
    loop {
        match status_receiver
            .recv()
            .await
            .map_err(|_| Error::insert_package("async export worker stopped without a result"))?
        {
            WorkerStatus::Ready => {}
            WorkerStatus::Finished(result) => return result,
        }
    }
}

async fn wait_for_finished_or_cancel(
    status_receiver: &async_channel::Receiver<WorkerStatus>,
    cancellation: &CancellationToken,
    operation_cancellation: &CancellationToken,
) -> Result<usize> {
    let finished = wait_for_finished(status_receiver).fuse();
    let cancelled = cancellation.cancelled().fuse();
    pin_mut!(finished, cancelled);
    select_biased! {
        _ = cancelled => {
            operation_cancellation.cancel();
            wait_for_finished(status_receiver).await
        },
        result = finished => result,
    }
}
