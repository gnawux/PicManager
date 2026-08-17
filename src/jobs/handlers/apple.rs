use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::application::{Application, RequestContext, ServiceError, ServiceResult};
use crate::jobs::{EnqueueResult, HandlerFuture, Job, JobControl, JobFailure, JobHandler, NewJob};

use super::{ThumbnailJobPayload, enqueue_thumbnail};

// Warm the two timeline sizes. Maps and high-density previews remain on-demand so
// recovery does not monopolize CPU after a large Apple synchronization.
const PREVIEW_SIZES: [u32; 2] = [256, 512];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ApplePostprocessPayload {
    source_id: Option<i64>,
}

#[derive(Clone)]
pub struct ApplePostprocessJobHandler(pub Application);

pub async fn enqueue_apple_postprocess(
    application: &Application,
    context: &RequestContext,
    source_id: Option<i64>,
) -> ServiceResult<EnqueueResult> {
    if source_id.is_none() {
        if let Some(job_id) = sqlx::query_scalar(
            "SELECT id FROM application_jobs WHERE kind = 'apple_postprocess' \
             AND status IN ('queued', 'running', 'retry_wait') \
             AND json_extract(payload_json, '$.source_id') IS NULL ORDER BY id LIMIT 1",
        )
        .fetch_optional(application.pool())
        .await
        .map_err(crate::error::AppError::from)
        .map_err(ServiceError::from)?
        {
            return Ok(EnqueueResult {
                job: crate::jobs::get(application.pool(), job_id)
                    .await
                    .map_err(ServiceError::from)?,
                created: false,
            });
        }
    }
    let mut job = NewJob::new(
        "apple_postprocess",
        serde_json::to_value(ApplePostprocessPayload { source_id })
            .expect("Apple postprocess payload is serializable"),
    );
    job.priority = -20;
    job.max_attempts = 3;
    job.correlation_id = Some(context.request_id.to_string());
    job.idempotency_key = context.idempotency_key.as_deref().map(str::to_owned);
    crate::jobs::enqueue(application.pool(), &job)
        .await
        .map_err(ServiceError::from)
}

impl JobHandler for ApplePostprocessJobHandler {
    fn execute(&self, job: Job, control: JobControl) -> HandlerFuture {
        let application = self.0.clone();
        Box::pin(async move {
            let payload: ApplePostprocessPayload = serde_json::from_str(&job.payload_json)
                .map_err(|error| JobFailure::terminal("invalid_payload", error.to_string()))?;
            let rows: Vec<(i64, i64, String, i64)> = sqlx::query_as(
                "SELECT s.id, a.photo_id, original.path, p.render_revision \
                 FROM asset_sources s JOIN assets a ON a.id = s.asset_id \
                 JOIN photos p ON p.id = a.photo_id \
                 JOIN asset_variants original ON original.asset_id = a.id \
                    AND original.source_id = s.id AND original.role = 'original' \
                 LEFT JOIN derived_media_state d ON d.photo_id = p.id \
                 WHERE s.provider = 'apple_photos' AND s.sync_status = 'ready' \
                   AND (? IS NULL OR s.id = ?) \
                   AND (? IS NOT NULL OR s.local_metadata_at IS NULL \
                        OR d.thumbnail_status = 'pending') \
                 ORDER BY s.id",
            )
            .bind(payload.source_id)
            .bind(payload.source_id)
            .bind(payload.source_id)
            .fetch_all(application.pool())
            .await
            .map_err(|error| JobFailure::retryable("catalog_read_failed", error.to_string()))?;
            control
                .progress(0, Some(rows.len() as i64), Some("extracting_apple_metadata"))
                .await
                .map_err(progress_failure)?;
            let context = application
                .request_context(crate::application::CallerKind::InternalWorker)
                .with_request_id(job.correlation_id.unwrap_or_else(|| format!("job-{}", job.id)));
            let mut thumbnails_scheduled = 0_u64;
            let mut metadata_extracted = 0_u64;
            for (index, (source_id, photo_id, path, render_revision)) in rows.iter().enumerate() {
                if control.cancellation_requested().await.unwrap_or(false) {
                    return Ok(());
                }
                let metadata_path = PathBuf::from(path);
                let metadata = tokio::task::spawn_blocking(move || {
                    crate::metadata::extract_from_file(&metadata_path)
                })
                .await
                .map_err(|error| JobFailure::retryable("metadata_task_failed", error.to_string()))?;
                let mut tx = application.pool().begin().await
                    .map_err(|error| JobFailure::retryable("metadata_write_failed", error.to_string()))?;
                match metadata {
                    Ok(metadata) => {
                        sqlx::query(
                            "UPDATE photos SET camera = COALESCE(camera, ?), \
                             gps_lat = COALESCE(gps_lat, ?), gps_lon = COALESCE(gps_lon, ?), \
                             timezone_offset = COALESCE(timezone_offset, ?), \
                             exif_orientation = CASE WHEN exif_orientation = 1 THEN ? ELSE exif_orientation END \
                             WHERE id = ?",
                        )
                        .bind(metadata.camera)
                        .bind(metadata.gps_lat)
                        .bind(metadata.gps_lon)
                        .bind(metadata.timezone_offset)
                        .bind(i32::from(metadata.exif_orientation))
                        .bind(photo_id)
                        .execute(&mut *tx)
                        .await
                        .map_err(|error| JobFailure::retryable("metadata_write_failed", error.to_string()))?;
                        sqlx::query(
                            "UPDATE asset_sources SET local_metadata_at = datetime('now'), \
                             local_metadata_error = NULL, updated_at = datetime('now') WHERE id = ?",
                        )
                        .bind(source_id)
                        .execute(&mut *tx)
                        .await
                        .map_err(|error| JobFailure::retryable("metadata_write_failed", error.to_string()))?;
                        crate::album::organize::assign_month_in_transaction(&mut tx, *photo_id)
                            .await
                            .map_err(|error| JobFailure::retryable("album_update_failed", error.to_string()))?;
                        crate::album::organize::assign_camera_in_transaction(&mut tx, *photo_id)
                            .await
                            .map_err(|error| JobFailure::retryable("album_update_failed", error.to_string()))?;
                        metadata_extracted += 1;
                    }
                    Err(error) => {
                        sqlx::query(
                            "UPDATE asset_sources SET local_metadata_at = datetime('now'), \
                             local_metadata_error = ?, updated_at = datetime('now') WHERE id = ?",
                        )
                        .bind(error.to_string())
                        .bind(source_id)
                        .execute(&mut *tx)
                        .await
                        .map_err(|error| JobFailure::retryable("metadata_write_failed", error.to_string()))?;
                    }
                }
                tx.commit().await
                    .map_err(|error| JobFailure::retryable("metadata_write_failed", error.to_string()))?;

                for size in PREVIEW_SIZES {
                    enqueue_thumbnail(
                        &application,
                        &context,
                        ThumbnailJobPayload {
                            photo_id: *photo_id,
                            render_revision: *render_revision,
                            size: Some(size),
                        },
                    )
                    .await
                    .map_err(service_failure)?;
                    thumbnails_scheduled += 1;
                }
                control
                    .progress(
                        (index + 1) as i64,
                        Some(rows.len() as i64),
                        Some("extracting_apple_metadata"),
                    )
                    .await
                    .map_err(progress_failure)?;
            }
            control
                .set_result(&serde_json::json!({
                    "sources_processed": rows.len(),
                    "metadata_extracted": metadata_extracted,
                    "thumbnails_scheduled": thumbnails_scheduled,
                }))
                .await
                .map_err(result_failure)?;
            Ok(())
        })
    }
}

fn service_failure(error: ServiceError) -> JobFailure {
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
    use crate::config::Config;
    use crate::jobs::{WorkerConfig, WorkerRuntime, get};
    use sqlx::sqlite::SqlitePoolOptions;

    #[tokio::test]
    async fn postprocess_extracts_exif_and_generates_timeline_thumbnails() {
        let directory = tempfile::tempdir().unwrap();
        let original = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/with_exif.jpg");
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO photos (id, path, sha256, format, import_status, render_revision) \
             VALUES (1, ?, 'apple-exif', 'jpeg', 'imported', 1)",
        ).bind(original.to_string_lossy().as_ref()).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO assets (id, photo_id) VALUES (1, 1)")
            .execute(&pool).await.unwrap();
        let source_id: i64 = sqlx::query_scalar(
            "INSERT INTO asset_sources (asset_id, provider, external_id, sync_status) \
             VALUES (1, 'apple_photos', 'postprocess-test', 'ready') RETURNING id",
        ).fetch_one(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO asset_variants (asset_id, source_id, role, path, content_sha256, mime_type) \
             VALUES (1, ?, 'original', ?, 'apple-exif', 'image/jpeg')",
        ).bind(source_id).bind(original.to_string_lossy().as_ref()).execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO derived_media_state (photo_id, render_revision, thumbnail_status, face_status) \
             VALUES (1, 1, 'pending', 'pending')",
        ).execute(&pool).await.unwrap();
        let mut config = Config::default();
        config.thumb_cache_dir = directory.path().join("cache");
        let application = Application::new(pool.clone(), config.clone());
        let context = application.request_context(crate::application::CallerKind::InternalWorker);
        let job = enqueue_apple_postprocess(&application, &context, Some(source_id))
            .await.unwrap().job;
        let worker = WorkerRuntime::new(
            pool.clone(),
            crate::jobs::handlers::registry(application),
            WorkerConfig {
                concurrency: 1,
                poll_interval: Duration::from_millis(10),
                shutdown_timeout: Duration::from_secs(2),
                ..Default::default()
            },
            "apple-postprocess-test",
        ).start();
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let completed = get(&pool, job.id).await.unwrap().status == "succeeded";
                let thumbnails: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM application_jobs WHERE kind = 'thumbnail' AND status = 'succeeded'",
                ).fetch_one(&pool).await.unwrap();
                if completed && thumbnails == PREVIEW_SIZES.len() as i64 { break; }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.unwrap();
        worker.shutdown().await;

        let metadata: (Option<String>, Option<f64>, Option<f64>) = sqlx::query_as(
            "SELECT camera, gps_lat, gps_lon FROM photos WHERE id = 1",
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(metadata.0.as_deref(), Some("Apple iPhone 15 Pro"));
        assert!(metadata.1.is_some() && metadata.2.is_some());
        for size in PREVIEW_SIZES {
            assert!(crate::derived::sized_thumbnail_cache_path(
                &config.thumb_cache_dir, 1, 1, size,
            ).is_file());
        }
        let camera_album: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM photo_albums pa JOIN albums a ON a.id = pa.album_id \
             WHERE pa.photo_id = 1 AND a.kind = 'camera' AND a.name = 'Apple iPhone 15 Pro'",
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(camera_album, 1);
    }
}
