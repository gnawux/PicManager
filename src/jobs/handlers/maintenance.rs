use serde::{Deserialize, Serialize};

use crate::application::{Application, CallerKind, RequestContext, ServiceError, ServiceResult};
use crate::jobs::{EnqueueResult, HandlerFuture, Job, JobControl, JobFailure, JobHandler, NewJob};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DedupScanPayload {
    full: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DerivedMaintenancePayload {
    photo_ids: Option<Vec<i64>>,
}

#[derive(Clone)]
pub struct DedupScanJobHandler(pub Application);

#[derive(Clone)]
pub struct DerivedMaintenanceJobHandler(pub Application);

pub async fn enqueue_dedup_scan(
    application: &Application,
    context: &RequestContext,
    full: bool,
) -> ServiceResult<EnqueueResult> {
    enqueue_job(
        application,
        context,
        "dedup_scan",
        serde_json::to_value(DedupScanPayload { full }).unwrap(),
    )
    .await
}

pub async fn enqueue_derived_maintenance(
    application: &Application,
    context: &RequestContext,
    photo_ids: Option<Vec<i64>>,
) -> ServiceResult<EnqueueResult> {
    enqueue_job(
        application,
        context,
        "derived_maintenance",
        serde_json::to_value(DerivedMaintenancePayload { photo_ids }).unwrap(),
    )
    .await
}

async fn enqueue_job(
    application: &Application,
    context: &RequestContext,
    kind: &str,
    payload: serde_json::Value,
) -> ServiceResult<EnqueueResult> {
    let mut job = NewJob::new(kind, payload);
    job.correlation_id = Some(context.request_id.to_string());
    job.idempotency_key = context.idempotency_key.as_deref().map(str::to_owned);
    crate::jobs::enqueue(application.pool(), &job)
        .await
        .map_err(ServiceError::from)
}

impl JobHandler for DedupScanJobHandler {
    fn execute(&self, job: Job, control: JobControl) -> HandlerFuture {
        let application = self.0.clone();
        Box::pin(async move {
            let payload: DedupScanPayload = serde_json::from_str(&job.payload_json)
                .map_err(|error| JobFailure::terminal("invalid_payload", error.to_string()))?;
            control
                .progress(0, Some(1), Some("scanning_duplicates"))
                .await
                .map_err(progress_failure)?;
            let context = application
                .request_context(CallerKind::InternalWorker)
                .with_request_id(
                    job.correlation_id
                        .unwrap_or_else(|| format!("job-{}", job.id)),
                );
            let groups = application
                .dedup()
                .scan(&context, payload.full)
                .await
                .map_err(service_failure)?;
            control
                .progress(1, Some(1), Some("completed"))
                .await
                .map_err(progress_failure)?;
            control
                .set_result(&serde_json::json!({"groups_created": groups}))
                .await
                .map_err(result_failure)?;
            Ok(())
        })
    }
}

impl JobHandler for DerivedMaintenanceJobHandler {
    fn execute(&self, job: Job, control: JobControl) -> HandlerFuture {
        let application = self.0.clone();
        Box::pin(async move {
            let payload: DerivedMaintenancePayload = serde_json::from_str(&job.payload_json)
                .map_err(|error| JobFailure::terminal("invalid_payload", error.to_string()))?;
            let rows: Vec<(i64, i64)> = if let Some(mut ids) = payload.photo_ids {
                ids.sort_unstable();
                ids.dedup();
                let mut rows = Vec::new();
                for id in ids {
                    if let Some(row) = sqlx::query_as(
                        "SELECT id, render_revision FROM photos \
                         WHERE id = ? AND import_status = 'imported'",
                    )
                    .bind(id)
                    .fetch_optional(application.pool())
                    .await
                    .map_err(|error| {
                        JobFailure::retryable("catalog_read_failed", error.to_string())
                    })? {
                        rows.push(row);
                    }
                }
                rows
            } else {
                sqlx::query_as(
                    "SELECT p.id, p.render_revision FROM photos p \
                     JOIN derived_media_state d ON d.photo_id = p.id \
                     WHERE p.import_status = 'imported' \
                       AND (d.thumbnail_status = 'pending' OR d.face_status = 'pending') \
                     ORDER BY p.id",
                )
                .fetch_all(application.pool())
                .await
                .map_err(|error| JobFailure::retryable("catalog_read_failed", error.to_string()))?
            };
            control
                .progress(0, Some(rows.len() as i64), Some("scheduling_derived_media"))
                .await
                .map_err(progress_failure)?;
            let worker_context = application
                .request_context(CallerKind::InternalWorker)
                .with_request_id(
                    job.correlation_id
                        .unwrap_or_else(|| format!("job-{}", job.id)),
                );
            let mut face_ids = Vec::with_capacity(rows.len());
            for (index, (photo_id, render_revision)) in rows.iter().enumerate() {
                super::enqueue_thumbnail(
                    &application,
                    &worker_context,
                    super::ThumbnailJobPayload {
                        photo_id: *photo_id,
                        render_revision: *render_revision,
                        size: None,
                    },
                )
                .await
                .map_err(service_failure)?;
                face_ids.push(*photo_id);
                control
                    .progress(
                        (index + 1) as i64,
                        Some(rows.len() as i64),
                        Some("scheduling_derived_media"),
                    )
                    .await
                    .map_err(progress_failure)?;
            }
            if !face_ids.is_empty() {
                super::enqueue_face_analysis(&application, &worker_context, Some(face_ids.clone()))
                    .await
                    .map_err(service_failure)?;
            }
            control
                .set_result(&serde_json::json!({"photos_scheduled": face_ids.len()}))
                .await
                .map_err(result_failure)?;
            Ok(())
        })
    }
}

fn service_failure(error: crate::application::ServiceError) -> JobFailure {
    JobFailure {
        code: "service_error".into(),
        message: error.message,
        details: error.details,
        retryable: error.retryable,
    }
}

fn progress_failure(error: crate::error::AppError) -> JobFailure {
    JobFailure::retryable("progress_write_failed", error.to_string())
}

fn result_failure(error: crate::error::AppError) -> JobFailure {
    JobFailure::retryable("result_write_failed", error.to_string())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::application::CallerKind;
    use crate::config::Config;
    use crate::jobs::{WorkerConfig, WorkerRuntime, get, list};
    use sqlx::sqlite::SqlitePoolOptions;

    #[tokio::test]
    async fn maintenance_jobs_persist_results_and_schedule_revision_work() {
        let directory = tempfile::tempdir().unwrap();
        let image_path = directory.path().join("photo.png");
        image::DynamicImage::new_rgb8(16, 16)
            .save(&image_path)
            .unwrap();
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let photo_id: i64 = sqlx::query_scalar(
            "INSERT INTO photos (path, sha256, format, import_status) \
             VALUES (?, 'maintenance-photo', 'png', 'imported') RETURNING id",
        )
        .bind(image_path.to_string_lossy().as_ref())
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO derived_media_state \
             (photo_id, render_revision, thumbnail_status, face_status) \
             VALUES (?, 0, 'pending', 'pending')",
        )
        .bind(photo_id)
        .execute(&pool)
        .await
        .unwrap();
        let mut config = Config::default();
        config.thumb_cache_dir = directory.path().join("cache");
        let application = Application::new(pool.clone(), config);
        let context = application.request_context(CallerKind::Cli);
        let dedup = enqueue_dedup_scan(&application, &context, false)
            .await
            .unwrap()
            .job;
        let maintenance = enqueue_derived_maintenance(&application, &context, None)
            .await
            .unwrap()
            .job;
        let worker = WorkerRuntime::new(
            pool.clone(),
            crate::jobs::handlers::registry(application),
            WorkerConfig {
                concurrency: 1,
                poll_interval: Duration::from_millis(10),
                shutdown_timeout: Duration::from_secs(2),
                ..Default::default()
            },
            "maintenance-test",
        )
        .start();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if get(&pool, dedup.id).await.unwrap().status == "succeeded"
                    && get(&pool, maintenance.id).await.unwrap().status == "succeeded"
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        worker.shutdown().await;
        assert_eq!(
            get(&pool, dedup.id)
                .await
                .unwrap()
                .result()
                .unwrap()
                .unwrap()["groups_created"],
            0,
        );
        assert_eq!(
            list(&pool, None, Some("thumbnail"), None, 10)
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            list(&pool, None, Some("face_analysis"), None, 10)
                .await
                .unwrap()
                .len(),
            1
        );
    }
}
