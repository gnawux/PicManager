use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::{Application, RequestContext, ServiceError, ServiceErrorCode, ServiceResult};

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct PhotoMetadataUpdate {
    pub taken_at: Option<String>,
    pub timezone_offset: Option<i64>,
    pub rotation_delta: Option<i32>,
    pub flip_h_toggle: Option<bool>,
    pub flip_v_toggle: Option<bool>,
}

impl PhotoMetadataUpdate {
    pub fn changes_display_transform(&self) -> bool {
        self.rotation_delta.is_some()
            || self.flip_h_toggle == Some(true)
            || self.flip_v_toggle == Some(true)
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct MetadataUpdateResult {
    pub updated: u64,
}

#[derive(Clone)]
pub struct PhotoMetadataService {
    application: Application,
}

impl PhotoMetadataService {
    pub(crate) fn new(application: Application) -> Self {
        Self { application }
    }

    pub async fn update_one(
        &self,
        context: &RequestContext,
        photo_id: i64,
        update: PhotoMetadataUpdate,
    ) -> ServiceResult<MetadataUpdateResult> {
        let result = self.update_many(context, &[photo_id], update).await?;
        if result.updated == 0 {
            return Err(ServiceError::new(
                ServiceErrorCode::NotFound,
                format!("photo {photo_id}"),
            ));
        }
        Ok(result)
    }

    pub async fn update_many(
        &self,
        context: &RequestContext,
        photo_ids: &[i64],
        update: PhotoMetadataUpdate,
    ) -> ServiceResult<MetadataUpdateResult> {
        self.authorize(context)?;
        let ids: BTreeSet<i64> = photo_ids.iter().copied().collect();
        if ids.is_empty() {
            return Ok(MetadataUpdateResult { updated: 0 });
        }

        let transform_changed = update.changes_display_transform();
        let mut tx = self
            .application
            .pool()
            .begin()
            .await
            .map_err(crate::error::AppError::from)
            .map_err(ServiceError::from)?;
        let mut changed_ids = Vec::new();

        for photo_id in ids {
            let exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM photos WHERE id = ?)")
                    .bind(photo_id)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(crate::error::AppError::from)
                    .map_err(ServiceError::from)?;
            if !exists {
                continue;
            }

            if let Some(taken_at) = &update.taken_at {
                sqlx::query("UPDATE photos SET taken_at = ? WHERE id = ?")
                    .bind(taken_at)
                    .bind(photo_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(crate::error::AppError::from)
                    .map_err(ServiceError::from)?;
            }
            if let Some(timezone_offset) = update.timezone_offset {
                sqlx::query("UPDATE photos SET timezone_offset = ? WHERE id = ?")
                    .bind(timezone_offset)
                    .bind(photo_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(crate::error::AppError::from)
                    .map_err(ServiceError::from)?;
            }
            if let Some(rotation_delta) = update.rotation_delta {
                sqlx::query(
                    "UPDATE photos SET rotation = ((rotation + ?) % 360 + 360) % 360 WHERE id = ?",
                )
                .bind(rotation_delta)
                .bind(photo_id)
                .execute(&mut *tx)
                .await
                .map_err(crate::error::AppError::from)
                .map_err(ServiceError::from)?;
            }
            if update.flip_h_toggle == Some(true) {
                sqlx::query("UPDATE photos SET flip_h = 1 - flip_h WHERE id = ?")
                    .bind(photo_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(crate::error::AppError::from)
                    .map_err(ServiceError::from)?;
            }
            if update.flip_v_toggle == Some(true) {
                sqlx::query("UPDATE photos SET flip_v = 1 - flip_v WHERE id = ?")
                    .bind(photo_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(crate::error::AppError::from)
                    .map_err(ServiceError::from)?;
            }
            if transform_changed {
                crate::derived::invalidate_in_transaction(&mut tx, photo_id)
                    .await
                    .map_err(ServiceError::from)?;
            }
            changed_ids.push(photo_id);
        }

        tx.commit()
            .await
            .map_err(crate::error::AppError::from)
            .map_err(ServiceError::from)?;

        if transform_changed && !changed_ids.is_empty() {
            if let Err(error) = crate::jobs::handlers::enqueue_derived_maintenance(
                &self.application,
                context,
                Some(changed_ids.clone()),
            )
            .await
            {
                tracing::error!("failed to enqueue derived-media maintenance: {error}");
            }
        }

        Ok(MetadataUpdateResult {
            updated: changed_ids.len() as u64,
        })
    }

    fn authorize(&self, context: &RequestContext) -> ServiceResult<()> {
        if context.library != *self.application.library() {
            return Err(ServiceError::new(
                ServiceErrorCode::Unauthorized,
                "Request context does not belong to this library",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{application::CallerKind, config::Config};
    use sqlx::sqlite::SqlitePoolOptions;

    async fn application() -> Application {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO photos (id, path, sha256, format, import_status) \
             VALUES (1, '/one.jpg', 'one', 'jpeg', 'imported'), \
                    (2, '/two.jpg', 'two', 'jpeg', 'imported')",
        )
        .execute(&pool)
        .await
        .unwrap();
        Application::new(pool, Config::default())
    }

    #[tokio::test]
    async fn transform_and_derived_invalidation_commit_together() {
        let app = application().await;
        let context = app.request_context(CallerKind::LocalWeb);
        let result = app
            .metadata()
            .update_one(
                &context,
                1,
                PhotoMetadataUpdate {
                    rotation_delta: Some(90),
                    flip_h_toggle: Some(true),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let row: (i64, i64, i64) =
            sqlx::query_as("SELECT rotation, flip_h, render_revision FROM photos WHERE id = 1")
                .fetch_one(app.pool())
                .await
                .unwrap();
        let pending: (String, String) = sqlx::query_as(
            "SELECT thumbnail_status, face_status FROM derived_media_state WHERE photo_id = 1",
        )
        .fetch_one(app.pool())
        .await
        .unwrap();
        assert_eq!(result.updated, 1);
        assert_eq!(row, (90, 1, 1));
        assert_eq!(pending, ("pending".into(), "pending".into()));
    }

    #[tokio::test]
    async fn batch_deduplicates_ids_and_reports_only_existing_photos() {
        let app = application().await;
        let context = app.request_context(CallerKind::Cli);
        let result = app
            .metadata()
            .update_many(
                &context,
                &[1, 1, 2, 404],
                PhotoMetadataUpdate {
                    timezone_offset: Some(480),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(result.updated, 2);
    }

    #[tokio::test]
    async fn single_missing_photo_is_not_found() {
        let app = application().await;
        let context = app.request_context(CallerKind::LocalWeb);
        let error = app
            .metadata()
            .update_one(&context, 404, PhotoMetadataUpdate::default())
            .await
            .unwrap_err();
        assert_eq!(error.code, ServiceErrorCode::NotFound);
    }
}
