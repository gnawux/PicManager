use std::sync::atomic::Ordering::Relaxed;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::application::{Application, CallerKind, ImportCommand, ServiceError};
use crate::importer::SharedImportProgress;
use crate::jobs::{HandlerFuture, Job, JobControl, JobFailure, JobHandler};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImportJobResult {
    pub total: usize,
    pub imported: usize,
    pub skipped: usize,
    pub errors: usize,
    pub total_files: usize,
    pub remaining: usize,
}

#[derive(Clone)]
pub struct ImportJobHandler {
    application: Application,
}

impl ImportJobHandler {
    pub fn new(application: Application) -> Self {
        Self { application }
    }
}

impl JobHandler for ImportJobHandler {
    fn execute(&self, job: Job, control: JobControl) -> HandlerFuture {
        let application = self.application.clone();
        Box::pin(async move {
            if job.payload_version != 1 {
                return Err(JobFailure::terminal(
                    "unsupported_payload_version",
                    format!("Unsupported import payload version {}", job.payload_version),
                ));
            }
            let command: ImportCommand =
                serde_json::from_str(&job.payload_json).map_err(|error| {
                    JobFailure::terminal(
                        "invalid_payload",
                        format!("Invalid import payload: {error}"),
                    )
                })?;
            let progress = SharedImportProgress::default();
            let worker_progress = progress.clone();
            let context = application
                .request_context(CallerKind::InternalWorker)
                .with_request_id(
                    job.correlation_id
                        .clone()
                        .unwrap_or_else(|| format!("job-{}", job.id)),
                );
            let service = application.imports();
            let execution = service.execute(&context, command, worker_progress);
            tokio::pin!(execution);

            let batch = loop {
                tokio::select! {
                    result = &mut execution => {
                        break result.map_err(service_failure)?;
                    }
                    _ = tokio::time::sleep(Duration::from_millis(250)) => {
                        let total = progress.total.load(Relaxed);
                        let completed = progress.processed.load(Relaxed);
                        control
                            .progress(completed as i64, Some(total as i64), Some("importing"))
                            .await
                            .map_err(|error| JobFailure::retryable("progress_write_failed", error.to_string()))?;
                    }
                }
            };

            control
                .progress(
                    batch.summary.total as i64,
                    Some(batch.summary.total as i64),
                    Some("completed"),
                )
                .await
                .map_err(|error| {
                    JobFailure::retryable("progress_write_failed", error.to_string())
                })?;
            let result = ImportJobResult {
                total: batch.summary.total,
                imported: batch.summary.imported,
                skipped: batch.summary.skipped,
                errors: batch.summary.errors,
                total_files: batch.total_files,
                remaining: batch.remaining,
            };
            control
                .set_result(&serde_json::to_value(result).expect("import result is serializable"))
                .await
                .map_err(|error| JobFailure::retryable("result_write_failed", error.to_string()))?;
            Ok(())
        })
    }
}

fn service_failure(error: ServiceError) -> JobFailure {
    let code = serde_json::to_value(error.code)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "service_error".into());
    JobFailure {
        code,
        message: error.message,
        details: error.details,
        retryable: error.retryable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::jobs::{WorkerConfig, WorkerRuntime, get};
    use sqlx::sqlite::SqlitePoolOptions;

    #[tokio::test]
    async fn durable_import_persists_summary() {
        let source = tempfile::tempdir().unwrap();
        let library = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("ignored.txt"), b"not a photo").unwrap();
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let mut config = Config::default();
        config.library_path = library.path().to_path_buf();
        let application = Application::new(pool.clone(), config);
        let context = application.request_context(CallerKind::Cli);
        let queued = application
            .imports()
            .enqueue(&context, ImportCommand::directory(source.path(), true))
            .await
            .unwrap();
        let worker = WorkerRuntime::new(
            pool.clone(),
            crate::jobs::handlers::registry(application),
            WorkerConfig {
                concurrency: 1,
                poll_interval: Duration::from_millis(10),
                shutdown_timeout: Duration::from_secs(2),
                ..Default::default()
            },
            "import-test",
        )
        .start();

        let completed = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let current = get(&pool, queued.job.id).await.unwrap();
                if current.status == "succeeded" {
                    break current;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(worker.shutdown().await);
        let result: ImportJobResult =
            serde_json::from_value(completed.result().unwrap().unwrap()).unwrap();
        assert_eq!(result.total_files, 0);
        assert_eq!(result.errors, 0);
    }
}
