use std::sync::atomic::Ordering::Relaxed;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::album::location::SharedGeoProgress;
use crate::application::{Application, RequestContext, ServiceError, ServiceResult};
use crate::jobs::{EnqueueResult, HandlerFuture, Job, JobControl, JobFailure, JobHandler, NewJob};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhotoAnalysisPayload {
    pub photo_ids: Option<Vec<i64>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GeocodePayload {
    #[serde(default)]
    pub refresh_names: bool,
}

#[derive(Clone)]
pub struct FaceAnalysisJobHandler(pub Application);

#[derive(Clone)]
pub struct AnimalAnalysisJobHandler(pub Application);

#[derive(Clone)]
pub struct GeocodeJobHandler(pub Application);

pub async fn enqueue_face_analysis(
    application: &Application,
    context: &RequestContext,
    photo_ids: Option<Vec<i64>>,
) -> ServiceResult<EnqueueResult> {
    enqueue_analysis(application, context, "face_analysis", photo_ids).await
}

pub async fn enqueue_animal_analysis(
    application: &Application,
    context: &RequestContext,
    photo_ids: Option<Vec<i64>>,
) -> ServiceResult<EnqueueResult> {
    enqueue_analysis(application, context, "animal_analysis", photo_ids).await
}

async fn enqueue_analysis(
    application: &Application,
    context: &RequestContext,
    kind: &str,
    photo_ids: Option<Vec<i64>>,
) -> ServiceResult<EnqueueResult> {
    let mut job = NewJob::new(
        kind,
        serde_json::to_value(PhotoAnalysisPayload { photo_ids })
            .expect("analysis payload is serializable"),
    );
    job.correlation_id = Some(context.request_id.to_string());
    job.idempotency_key = context.idempotency_key.as_deref().map(str::to_owned);
    crate::jobs::enqueue(application.pool(), &job)
        .await
        .map_err(ServiceError::from)
}

pub async fn enqueue_geocode(
    application: &Application,
    context: &RequestContext,
) -> ServiceResult<EnqueueResult> {
    enqueue_geocode_with_policy(application, context, false).await
}

pub async fn enqueue_geo_name_normalization(
    application: &Application,
    context: &RequestContext,
) -> ServiceResult<EnqueueResult> {
    enqueue_geocode_with_policy(application, context, true).await
}

async fn enqueue_geocode_with_policy(
    application: &Application,
    context: &RequestContext,
    refresh_names: bool,
) -> ServiceResult<EnqueueResult> {
    let mut job = NewJob::new(
        "geocode",
        serde_json::to_value(GeocodePayload { refresh_names })
            .expect("geocode payload is serializable"),
    );
    job.correlation_id = Some(context.request_id.to_string());
    job.idempotency_key = context.idempotency_key.as_deref().map(str::to_owned);
    crate::jobs::enqueue(application.pool(), &job)
        .await
        .map_err(ServiceError::from)
}

impl JobHandler for FaceAnalysisJobHandler {
    fn execute(&self, job: Job, control: JobControl) -> HandlerFuture {
        let application = self.0.clone();
        Box::pin(async move {
            let payload = analysis_payload(&job)?;
            let ids = resolve_photo_ids(&application, payload.photo_ids).await?;
            control
                .progress(0, Some(ids.len() as i64), Some("analyzing_faces"))
                .await
                .map_err(progress_failure)?;
            for (index, photo_id) in ids.iter().enumerate() {
                if control.cancellation_requested().await.unwrap_or(false) {
                    return Ok(());
                }
                crate::face::job::reanalyze_one_photo(application.pool(), *photo_id).await;
                control
                    .progress(
                        (index + 1) as i64,
                        Some(ids.len() as i64),
                        Some("analyzing_faces"),
                    )
                    .await
                    .map_err(progress_failure)?;
            }
            control
                .set_result(&serde_json::json!({"processed": ids.len()}))
                .await
                .map_err(result_failure)?;
            Ok(())
        })
    }
}

impl JobHandler for AnimalAnalysisJobHandler {
    fn execute(&self, job: Job, control: JobControl) -> HandlerFuture {
        let application = self.0.clone();
        Box::pin(async move {
            let payload = analysis_payload(&job)?;
            let ids = resolve_photo_ids(&application, payload.photo_ids).await?;
            control
                .progress(0, Some(ids.len() as i64), Some("analyzing_animals"))
                .await
                .map_err(progress_failure)?;
            for (index, photo_id) in ids.iter().enumerate() {
                if control.cancellation_requested().await.unwrap_or(false) {
                    return Ok(());
                }
                let path: Option<String> = sqlx::query_scalar(
                    "SELECT COALESCE(dv.path, p.path) FROM photos p \
                     LEFT JOIN assets a ON a.photo_id = p.id \
                     LEFT JOIN asset_variants dv ON dv.id = a.display_variant_id \
                     WHERE p.id = ? AND p.import_status = 'imported'",
                )
                .bind(photo_id)
                .fetch_optional(application.pool())
                .await
                .map_err(|error| JobFailure::retryable("catalog_read_failed", error.to_string()))?;
                if let Some(path) = path {
                    sqlx::query("DELETE FROM animals WHERE photo_id = ?")
                        .bind(photo_id)
                        .execute(application.pool())
                        .await
                        .map_err(|error| {
                            JobFailure::retryable("animal_reset_failed", error.to_string())
                        })?;
                    if let Ok(image) = crate::image_open::open_image(std::path::Path::new(&path)) {
                        crate::animal::detect_and_save(application.pool(), *photo_id, &image).await;
                    }
                }
                control
                    .progress(
                        (index + 1) as i64,
                        Some(ids.len() as i64),
                        Some("analyzing_animals"),
                    )
                    .await
                    .map_err(progress_failure)?;
            }
            control
                .set_result(&serde_json::json!({"processed": ids.len()}))
                .await
                .map_err(result_failure)?;
            Ok(())
        })
    }
}

impl JobHandler for GeocodeJobHandler {
    fn execute(&self, job: Job, control: JobControl) -> HandlerFuture {
        let application = self.0.clone();
        Box::pin(async move {
            let payload: GeocodePayload = serde_json::from_str(&job.payload_json)
                .map_err(|error| JobFailure::terminal("invalid_payload", error.to_string()))?;
            let progress = SharedGeoProgress::default();
            let execution = async {
                if payload.refresh_names {
                    crate::album::location::normalize_geo_names_with_progress(
                        application.pool(),
                        progress.clone(),
                    )
                    .await
                } else {
                    crate::album::location::group_by_location_with_progress(
                        application.pool(),
                        progress.clone(),
                    )
                    .await
                }
            };
            tokio::pin!(execution);
            let mut last_published = None;
            loop {
                tokio::select! {
                    result = &mut execution => {
                        result.map_err(|error| JobFailure::retryable("geocode_failed", error.to_string()))?;
                        break;
                    }
                    _ = tokio::time::sleep(Duration::from_secs(2)) => {
                        if control.cancellation_requested().await.unwrap_or(false) {
                            progress.cancellation_requested.store(true, Relaxed);
                        }
                        let snapshot = (
                            progress.processed.load(Relaxed),
                            progress.total.load(Relaxed),
                        );
                        if last_published != Some(snapshot) {
                            publish_geo_progress(&control, snapshot.0, snapshot.1, "geocoding").await?;
                            last_published = Some(snapshot);
                        }
                    }
                }
            }
            let total = progress.total.load(Relaxed);
            let processed = progress.processed.load(Relaxed);
            publish_geo_progress(&control, processed, total, "completed").await?;
            control
                .set_result(&serde_json::json!({
                    "processed_coordinates": processed,
                    "total_coordinates": total,
                    "updated_coordinates": progress.updated.load(Relaxed),
                    "cache_hits": progress.cache_hits.load(Relaxed),
                    "provider_failures": progress.provider_failures.load(Relaxed),
                }))
                .await
                .map_err(result_failure)?;
            Ok(())
        })
    }
}

async fn publish_geo_progress(
    control: &JobControl,
    completed: usize,
    total: usize,
    stage: &str,
) -> Result<(), JobFailure> {
    let mut last_error = None;
    for attempt in 0..3 {
        match control
            .progress(completed as i64, Some(total as i64), Some(stage))
            .await
        {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = Some(error);
                tokio::time::sleep(Duration::from_millis(100 * (attempt + 1))).await;
            }
        }
    }
    Err(progress_failure(
        last_error.expect("progress retry records an error"),
    ))
}

fn analysis_payload(job: &Job) -> Result<PhotoAnalysisPayload, JobFailure> {
    serde_json::from_str(&job.payload_json)
        .map_err(|error| JobFailure::terminal("invalid_payload", error.to_string()))
}

async fn resolve_photo_ids(
    application: &Application,
    requested: Option<Vec<i64>>,
) -> Result<Vec<i64>, JobFailure> {
    if let Some(mut ids) = requested {
        ids.sort_unstable();
        ids.dedup();
        return Ok(ids);
    }
    sqlx::query_scalar("SELECT id FROM photos WHERE import_status = 'imported' ORDER BY id")
        .fetch_all(application.pool())
        .await
        .map_err(|error| JobFailure::retryable("catalog_read_failed", error.to_string()))
}

fn progress_failure(error: crate::error::AppError) -> JobFailure {
    JobFailure::retryable("progress_write_failed", error.to_string())
}

fn result_failure(error: crate::error::AppError) -> JobFailure {
    JobFailure::retryable("result_write_failed", error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::CallerKind;
    use crate::config::Config;
    use crate::jobs::{WorkerConfig, WorkerRuntime, get};
    use sqlx::sqlite::SqlitePoolOptions;

    #[tokio::test]
    async fn analysis_handlers_complete_empty_scopes_durably() {
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let application = Application::new(pool.clone(), Config::default());
        let context = application.request_context(CallerKind::Cli);
        let face = enqueue_face_analysis(&application, &context, Some(vec![]))
            .await
            .unwrap()
            .job;
        let animal = enqueue_animal_analysis(&application, &context, Some(vec![]))
            .await
            .unwrap()
            .job;
        let geo = enqueue_geocode(&application, &context).await.unwrap().job;
        let worker = WorkerRuntime::new(
            pool.clone(),
            crate::jobs::handlers::registry(application),
            WorkerConfig {
                concurrency: 2,
                poll_interval: Duration::from_millis(10),
                shutdown_timeout: Duration::from_secs(2),
                ..Default::default()
            },
            "analysis-test",
        )
        .start();

        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let mut statuses = Vec::new();
                for id in [face.id, animal.id, geo.id] {
                    let job = get(&pool, id).await.unwrap();
                    statuses.push((job.status, job.error_message));
                }
                if statuses.iter().all(|(status, _)| status == "succeeded") {
                    break;
                }
                if statuses.iter().all(|(status, _)| {
                    matches!(status.as_str(), "succeeded" | "failed" | "cancelled")
                }) {
                    panic!("analysis jobs terminated unsuccessfully: {statuses:?}");
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(worker.shutdown().await);
    }
}
