use serde::{Deserialize, Serialize};

use crate::application::RequestContext;
use crate::derived::{
    generate_thumbnail, mark_thumbnail_ready, sized_thumbnail_cache_path, thumbnail_cache_path,
};
use crate::jobs::{EnqueueResult, HandlerFuture, Job, JobControl, JobFailure, JobHandler, NewJob};
use crate::orientation::OrientationMode;
use crate::{application::Application, jobs};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThumbnailJobPayload {
    pub photo_id: i64,
    pub render_revision: i64,
    pub size: Option<u32>,
}

#[derive(Clone)]
pub struct ThumbnailJobHandler {
    application: Application,
}

impl ThumbnailJobHandler {
    pub fn new(application: Application) -> Self {
        Self { application }
    }
}

pub async fn enqueue(
    application: &Application,
    context: &RequestContext,
    payload: ThumbnailJobPayload,
) -> crate::application::ServiceResult<EnqueueResult> {
    let suffix = payload
        .size
        .map_or_else(|| "default".into(), |size| format!("s{size}"));
    let mut job = NewJob::new(
        "thumbnail",
        serde_json::to_value(&payload).expect("thumbnail payload is serializable"),
    );
    job.correlation_id = Some(context.request_id.to_string());
    job.idempotency_key = Some(format!(
        "thumbnail:{}:r{}:{suffix}",
        payload.photo_id, payload.render_revision
    ));
    jobs::enqueue(application.pool(), &job)
        .await
        .map_err(crate::application::ServiceError::from)
}

impl JobHandler for ThumbnailJobHandler {
    fn execute(&self, job: Job, control: JobControl) -> HandlerFuture {
        let application = self.application.clone();
        Box::pin(async move {
            let payload: ThumbnailJobPayload = serde_json::from_str(&job.payload_json)
                .map_err(|error| JobFailure::terminal("invalid_payload", error.to_string()))?;
            let row: Option<(String, i32, i32, i32, i32, String, Option<i64>, i64)> =
                sqlx::query_as(
                    "SELECT COALESCE(dv.path, p.path), p.rotation, p.flip_h, p.flip_v, \
                            p.exif_orientation, COALESCE(vr.orientation_mode, 'legacy_unknown'), \
                            vr.display_orientation, p.render_revision \
                     FROM photos p \
                     LEFT JOIN assets a ON a.photo_id = p.id \
                     LEFT JOIN asset_variants dv ON dv.id = a.display_variant_id \
                     LEFT JOIN variant_renditions vr ON vr.variant_id = dv.id \
                     WHERE p.id = ?",
                )
                .bind(payload.photo_id)
                .fetch_optional(application.pool())
                .await
                .map_err(|error| JobFailure::retryable("catalog_read_failed", error.to_string()))?;
            let Some((path, rotation, flip_h, flip_v, exif, mode, display, current_revision)) = row
            else {
                return Err(JobFailure::terminal(
                    "photo_not_found",
                    "Photo was not found",
                ));
            };
            if current_revision != payload.render_revision {
                return Err(JobFailure::terminal(
                    "stale_revision",
                    "The photo display revision changed before thumbnail generation",
                ));
            }
            let cache_path = payload.size.map_or_else(
                || {
                    thumbnail_cache_path(
                        &application.config().thumb_cache_dir,
                        payload.photo_id,
                        payload.render_revision,
                    )
                },
                |size| {
                    sized_thumbnail_cache_path(
                        &application.config().thumb_cache_dir,
                        payload.photo_id,
                        payload.render_revision,
                        size,
                    )
                },
            );
            let render_size = payload.size.unwrap_or(application.config().thumb_size);
            control
                .progress(0, Some(1), Some("rendering"))
                .await
                .map_err(|error| {
                    JobFailure::retryable("progress_write_failed", error.to_string())
                })?;
            let bytes = tokio::task::spawn_blocking(move || {
                generate_thumbnail(
                    &path,
                    render_size,
                    OrientationMode::from_catalog(Some(&mode)),
                    display.map(|value| value as u8),
                    exif as u8,
                    rotation,
                    flip_h != 0,
                    flip_v != 0,
                )
            })
            .await
            .map_err(|error| JobFailure::retryable("renderer_interrupted", error.to_string()))?
            .map_err(|error| JobFailure::terminal("thumbnail_render_failed", error.to_string()))?;
            if let Some(parent) = cache_path.parent() {
                std::fs::create_dir_all(parent).map_err(|error| {
                    JobFailure::retryable("cache_directory_failed", error.to_string())
                })?;
            }
            let temporary = cache_path.with_extension(format!("jpg.job-{}", job.id));
            let intent_id: i64 = sqlx::query_scalar(
                "INSERT INTO filesystem_intents \
                 (kind, owner_kind, owner_id, target_path, staging_path) \
                 VALUES ('thumbnail', 'application_job', ?, ?, ?) RETURNING id",
            )
            .bind(job.id)
            .bind(cache_path.to_string_lossy().as_ref())
            .bind(temporary.to_string_lossy().as_ref())
            .fetch_one(application.pool())
            .await
            .map_err(|error| JobFailure::retryable("intent_write_failed", error.to_string()))?;
            if let Err(error) = std::fs::write(&temporary, bytes) {
                let _ = fail_intent(application.pool(), intent_id, &error.to_string()).await;
                return Err(JobFailure::retryable(
                    "cache_write_failed",
                    error.to_string(),
                ));
            }
            sqlx::query(
                "UPDATE filesystem_intents SET status = 'staged', updated_at = datetime('now') \
                 WHERE id = ?",
            )
            .bind(intent_id)
            .execute(application.pool())
            .await
            .map_err(|error| JobFailure::retryable("intent_write_failed", error.to_string()))?;
            if let Err(error) = std::fs::rename(&temporary, &cache_path) {
                let _ = fail_intent(application.pool(), intent_id, &error.to_string()).await;
                return Err(JobFailure::retryable(
                    "cache_commit_failed",
                    error.to_string(),
                ));
            }
            sqlx::query(
                "UPDATE filesystem_intents SET status = 'committed', completed_at = datetime('now'), \
                 updated_at = datetime('now') WHERE id = ?",
            )
            .bind(intent_id)
            .execute(application.pool())
            .await
            .map_err(|error| JobFailure::retryable("intent_write_failed", error.to_string()))?;
            mark_thumbnail_ready(
                application.pool(),
                payload.photo_id,
                payload.render_revision,
            )
            .await;
            control
                .progress(1, Some(1), Some("completed"))
                .await
                .map_err(|error| {
                    JobFailure::retryable("progress_write_failed", error.to_string())
                })?;
            control
                .set_result(&serde_json::json!({"cache_path": cache_path}))
                .await
                .map_err(|error| JobFailure::retryable("result_write_failed", error.to_string()))?;
            Ok(())
        })
    }
}

async fn fail_intent(
    pool: &sqlx::SqlitePool,
    intent_id: i64,
    error: &str,
) -> crate::error::Result<()> {
    sqlx::query(
        "UPDATE filesystem_intents SET status = 'failed', error = ?, updated_at = datetime('now') \
         WHERE id = ?",
    )
    .bind(error)
    .bind(intent_id)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::application::CallerKind;
    use crate::config::Config;
    use crate::jobs::{WorkerConfig, WorkerRuntime, get};
    use sqlx::sqlite::SqlitePoolOptions;

    #[tokio::test]
    async fn revision_keyed_jobs_are_idempotent_and_render_to_cache() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.png");
        image::DynamicImage::new_rgb8(40, 30).save(&source).unwrap();
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let photo_id: i64 = sqlx::query_scalar(
            "INSERT INTO photos (path, sha256, format, import_status) \
             VALUES (?, 'thumbnail-job', 'png', 'imported') RETURNING id",
        )
        .bind(source.to_string_lossy().as_ref())
        .fetch_one(&pool)
        .await
        .unwrap();
        let mut config = Config::default();
        config.thumb_cache_dir = directory.path().join("cache");
        let application = Application::new(pool.clone(), config);
        let context = application.request_context(CallerKind::LocalWeb);
        let payload = ThumbnailJobPayload {
            photo_id,
            render_revision: 0,
            size: Some(128),
        };
        let first = enqueue(&application, &context, payload.clone())
            .await
            .unwrap();
        let duplicate = enqueue(&application, &context, payload).await.unwrap();
        assert!(first.created);
        assert!(!duplicate.created);
        assert_eq!(first.job.id, duplicate.job.id);

        let worker = WorkerRuntime::new(
            pool.clone(),
            crate::jobs::handlers::registry(application.clone()),
            WorkerConfig {
                concurrency: 1,
                poll_interval: Duration::from_millis(10),
                shutdown_timeout: Duration::from_secs(2),
                ..Default::default()
            },
            "thumbnail-test",
        )
        .start();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if get(&pool, first.job.id).await.unwrap().status == "succeeded" {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(worker.shutdown().await);
        assert!(
            sized_thumbnail_cache_path(directory.path().join("cache").as_path(), photo_id, 0, 128)
                .exists()
        );
    }
}
