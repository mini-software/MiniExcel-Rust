use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures_core::Stream;
use futures_util::{FutureExt, StreamExt, pin_mut, select_biased};

use super::atomic::{AtomicCommitStage, insert_to_path_with_hook};
use super::donor::DonorBuilder;
use crate::{CancellationToken, DynamicRow, Error, InsertOptions, Result};

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
pub(crate) enum AsyncInsertStage {
    BeforePreflight,
    ReadyForRows,
    ZipCopy,
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

pub(crate) async fn insert_with_schema_async<S>(
    path: PathBuf,
    schema: Vec<String>,
    rows: S,
    options: InsertOptions,
    cancellation: CancellationToken,
) -> Result<usize>
where
    S: Stream<Item = Result<DynamicRow>>,
{
    insert_with_schema_async_with_hook(path, schema, rows, options, cancellation, Arc::new(|_| {}))
        .await
}

async fn insert_with_schema_async_with_hook<S>(
    path: PathBuf,
    schema: Vec<String>,
    rows: S,
    options: InsertOptions,
    cancellation: CancellationToken,
    phase_hook: Arc<dyn Fn(AsyncInsertStage) + Send + Sync>,
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

    std::thread::Builder::new().name("miniexcel-async-insert".to_owned()).spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_worker(
                &path,
                &schema,
                options,
                row_receiver,
                &worker_cancellation,
                &worker_operation_cancellation,
                &status_sender,
                &worker_hook,
            )
        }))
        .unwrap_or_else(|_| Err(Error::insert_package("async Insert worker panicked")));
        let _ = status_sender.send_blocking(WorkerStatus::Finished(result));
        worker_hook(AsyncInsertStage::Finished);
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
                let worker_status = worker_status.map_err(|_| Error::insert_package("async Insert worker stopped without a result"))?;
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
    options: InsertOptions,
    row_receiver: async_channel::Receiver<RowMessage>,
    cancellation: &CancellationToken,
    operation_cancellation: &CancellationToken,
    status_sender: &async_channel::Sender<WorkerStatus>,
    phase_hook: &Arc<dyn Fn(AsyncInsertStage) + Send + Sync>,
) -> Result<usize> {
    phase_hook(AsyncInsertStage::BeforePreflight);
    check_cancelled(cancellation, operation_cancellation)?;
    let mut ready_sent = false;
    insert_to_path_with_hook(
        path,
        options.write_options().sheet_name(),
        options.existing_sheet_policy(),
        options.target_relationship_policy(),
        || {
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
            DonorBuilder::from_dynamic_iter(schema, rows, options.write_options())
        },
        |stage| {
            let phase = match stage {
                AtomicCommitStage::Preflight => AsyncInsertStage::BeforePreflight,
                AtomicCommitStage::RowGeneration => {
                    if !ready_sent {
                        ready_sent = true;
                        phase_hook(AsyncInsertStage::ReadyForRows);
                        check_cancelled(cancellation, operation_cancellation)?;
                        status_sender
                            .send_blocking(WorkerStatus::Ready)
                            .map_err(|_| Error::cancelled())?;
                    }
                    return check_cancelled(cancellation, operation_cancellation);
                }
                AtomicCommitStage::ZipCopy | AtomicCommitStage::ZipFinish => {
                    AsyncInsertStage::ZipCopy
                }
                AtomicCommitStage::Validation => AsyncInsertStage::Validation,
                AtomicCommitStage::Commit => AsyncInsertStage::BeforeCommit,
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
        status = status => status.map_err(|_| Error::insert_package("async Insert worker stopped without a result")),
    }
}

async fn wait_for_finished(
    status_receiver: &async_channel::Receiver<WorkerStatus>,
) -> Result<usize> {
    loop {
        match status_receiver
            .recv()
            .await
            .map_err(|_| Error::insert_package("async Insert worker stopped without a result"))?
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

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Context, Poll};

    use futures_util::stream;
    use futures_util::task::noop_waker_ref;

    use super::*;
    use crate::writer::XlsxWriter;
    use crate::{CellValue, WriteOptions};

    #[test]
    fn cancellation_before_preflight_zip_validation_and_commit_preserves_source() {
        for stage in [
            AsyncInsertStage::BeforePreflight,
            AsyncInsertStage::ReadyForRows,
            AsyncInsertStage::ZipCopy,
            AsyncInsertStage::Validation,
            AsyncInsertStage::BeforeCommit,
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("book.xlsx");
            let original = source_package();
            std::fs::write(&path, &original).unwrap();
            let cancellation = CancellationToken::new();
            let hook_cancellation = cancellation.clone();
            let hook = Arc::new(move |observed| {
                if observed == stage {
                    hook_cancellation.cancel();
                }
            });
            let polls = Arc::new(AtomicUsize::new(0));
            let observed_polls = Arc::clone(&polls);
            let rows = stream::iter([Ok(row("Async", 2))]).inspect(move |_| {
                observed_polls.fetch_add(1, Ordering::SeqCst);
            });

            let result = futures_executor::block_on(insert_with_schema_async_with_hook(
                path.clone(),
                schema(),
                rows,
                InsertOptions::new().with_sheet_name("Async"),
                cancellation,
                hook,
            ));

            assert!(result.unwrap_err().is_cancelled(), "stage {stage:?}");
            assert_eq!(std::fs::read(&path).unwrap(), original, "stage {stage:?}");
            if matches!(stage, AsyncInsertStage::BeforePreflight | AsyncInsertStage::ReadyForRows) {
                assert_eq!(polls.load(Ordering::SeqCst), 0, "stage {stage:?}");
            }
            assert!(!std::fs::read_dir(directory.path()).unwrap().any(|entry| {
                entry.unwrap().file_name().to_string_lossy().starts_with(".miniexcel-")
            }));
        }
    }

    #[test]
    fn cancellation_during_row_generation_stops_stream_and_preserves_source() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("book.xlsx");
        let original = source_package();
        std::fs::write(&path, &original).unwrap();
        let cancellation = CancellationToken::new();
        let stream_cancellation = cancellation.clone();
        let polls = Arc::new(AtomicUsize::new(0));
        let observed_polls = Arc::clone(&polls);
        let rows = stream::poll_fn(move |_| {
            let poll = observed_polls.fetch_add(1, Ordering::SeqCst);
            if poll == 0 {
                Poll::Ready(Some(Ok(row("Async", 2))))
            } else {
                stream_cancellation.cancel();
                Poll::Pending
            }
        });

        let result = futures_executor::block_on(insert_with_schema_async(
            path.clone(),
            schema(),
            rows,
            InsertOptions::new().with_sheet_name("Async"),
            cancellation,
        ));

        assert!(result.unwrap_err().is_cancelled());
        assert_eq!(polls.load(Ordering::SeqCst), 2);
        assert_eq!(std::fs::read(path).unwrap(), original);
    }

    #[test]
    fn dropping_future_cancels_worker_and_releases_path_lock() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("book.xlsx");
        let original = source_package();
        std::fs::write(&path, &original).unwrap();
        let (stage_sender, stage_receiver) = std::sync::mpsc::channel();
        let hook = Arc::new(move |stage| {
            if matches!(stage, AsyncInsertStage::ReadyForRows | AsyncInsertStage::Finished) {
                stage_sender.send(stage).unwrap();
            }
        });
        let mut future = Box::pin(insert_with_schema_async_with_hook(
            path.clone(),
            schema(),
            stream::pending(),
            InsertOptions::new().with_sheet_name("Async"),
            CancellationToken::new(),
            hook,
        ));
        let mut context = Context::from_waker(noop_waker_ref());
        assert!(matches!(future.as_mut().poll(&mut context), Poll::Pending));
        assert_eq!(stage_receiver.recv().unwrap(), AsyncInsertStage::ReadyForRows);
        drop(future);
        assert_eq!(stage_receiver.recv().unwrap(), AsyncInsertStage::Finished);
        assert_eq!(std::fs::read(&path).unwrap(), original);

        let result = crate::MiniExcel::insert(
            path,
            &[row("Sync", 3)],
            &InsertOptions::new().with_sheet_name("Sync"),
        );
        assert_eq!(result.unwrap(), 1);
    }

    fn source_package() -> Vec<u8> {
        let mut writer = XlsxWriter::new();
        writer
            .add_rows(&[row("Existing", 1)], &WriteOptions::new().with_sheet_name("Data"))
            .unwrap();
        writer.save_to_bytes().unwrap()
    }

    fn row(name: &str, value: i64) -> DynamicRow {
        let mut row = DynamicRow::new();
        row.insert("Name".to_owned(), CellValue::String(name.to_owned()));
        row.insert("Value".to_owned(), CellValue::Int(value));
        row
    }

    fn schema() -> Vec<String> {
        vec!["Name".to_owned(), "Value".to_owned()]
    }
}
