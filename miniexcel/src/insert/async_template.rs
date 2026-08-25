use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures_util::{FutureExt, pin_mut, select_biased};
use serde_json::Value;

use super::atomic::{AtomicCommitStage, template_to_path_with_hook};
use crate::{CancellationToken, Error, Result, TemplateOptions};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AsyncTemplateStage {
    BeforePreflight,
    Rendering,
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

pub(crate) async fn fill_path_async(
    path: PathBuf,
    template_path: PathBuf,
    value: Value,
    options: TemplateOptions,
    cancellation: CancellationToken,
) -> Result<()> {
    fill_path_async_with_hook(path, template_path, value, options, cancellation, Arc::new(|_| {}))
        .await
}

async fn fill_path_async_with_hook(
    path: PathBuf,
    template_path: PathBuf,
    value: Value,
    options: TemplateOptions,
    cancellation: CancellationToken,
    phase_hook: Arc<dyn Fn(AsyncTemplateStage) + Send + Sync>,
) -> Result<()> {
    if cancellation.is_cancelled() {
        return Err(Error::cancelled());
    }
    let operation_cancellation = CancellationToken::new();
    let _guard = FutureCancellationGuard { cancellation: operation_cancellation.clone() };
    let (sender, receiver) = async_channel::bounded(1);
    let worker_cancellation = cancellation.clone();
    let worker_operation_cancellation = operation_cancellation.clone();
    let worker_hook = Arc::clone(&phase_hook);

    std::thread::Builder::new().name("miniexcel-async-template".to_owned()).spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_worker(
                &path,
                &template_path,
                &value,
                &options,
                &worker_cancellation,
                &worker_operation_cancellation,
                &worker_hook,
            )
        }))
        .unwrap_or_else(|_| Err(Error::template("async template worker panicked")));
        let _ = sender.send_blocking(result);
        worker_hook(AsyncTemplateStage::Finished);
    })?;

    let result = receiver.recv().fuse();
    let cancelled = cancellation.cancelled().fuse();
    pin_mut!(result, cancelled);
    select_biased! {
        _ = cancelled => {
            operation_cancellation.cancel();
            receiver.recv().await.map_err(|_| Error::template("async template worker stopped without a result"))?
        },
        result = result => result.map_err(|_| Error::template("async template worker stopped without a result"))?,
    }
}

fn run_worker(
    path: &Path,
    template_path: &Path,
    value: &Value,
    options: &TemplateOptions,
    cancellation: &CancellationToken,
    operation_cancellation: &CancellationToken,
    phase_hook: &Arc<dyn Fn(AsyncTemplateStage) + Send + Sync>,
) -> Result<()> {
    phase_hook(AsyncTemplateStage::BeforePreflight);
    check_cancelled(cancellation, operation_cancellation)?;
    template_to_path_with_hook(
        path,
        options.overwrite_file(),
        |writer| {
            phase_hook(AsyncTemplateStage::Rendering);
            crate::template::fill_path_value_to_writer(
                writer,
                template_path,
                value,
                options,
                &mut || check_cancelled(cancellation, operation_cancellation),
            )
        },
        |stage| {
            let phase = match stage {
                AtomicCommitStage::Preflight => AsyncTemplateStage::BeforePreflight,
                AtomicCommitStage::RowGeneration
                | AtomicCommitStage::ZipCopy
                | AtomicCommitStage::ZipFinish => AsyncTemplateStage::Rendering,
                AtomicCommitStage::Validation => AsyncTemplateStage::Validation,
                AtomicCommitStage::Commit => AsyncTemplateStage::BeforeCommit,
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rust_xlsxwriter::Workbook;
    use serde_json::json;

    use super::*;

    #[test]
    fn cancellation_at_each_template_phase_preserves_destination() {
        for stage in [
            AsyncTemplateStage::BeforePreflight,
            AsyncTemplateStage::Rendering,
            AsyncTemplateStage::Validation,
            AsyncTemplateStage::BeforeCommit,
        ] {
            let directory = tempfile::tempdir().unwrap();
            let template = directory.path().join("template.xlsx");
            let output = directory.path().join("output.xlsx");
            let mut workbook = Workbook::new();
            workbook.add_worksheet().write_string(0, 0, "{{value}}").unwrap();
            workbook.save(&template).unwrap();
            std::fs::write(&output, b"existing").unwrap();
            let cancellation = CancellationToken::new();
            let hook_cancellation = cancellation.clone();
            let hook = Arc::new(move |observed| {
                if observed == stage {
                    hook_cancellation.cancel();
                }
            });

            let result = futures_executor::block_on(fill_path_async_with_hook(
                output.clone(),
                template,
                json!({ "value": "Async" }),
                TemplateOptions::new().with_overwrite_file(true),
                cancellation,
                hook,
            ));

            assert!(matches!(result, Err(error) if error.is_cancelled()), "{stage:?}");
            assert_eq!(std::fs::read(&output).unwrap(), b"existing", "{stage:?}");
            assert!(!std::fs::read_dir(directory.path()).unwrap().any(|entry| {
                entry.unwrap().file_name().to_string_lossy().starts_with(".miniexcel-")
            }));
        }
    }
}
