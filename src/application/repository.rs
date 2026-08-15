use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::future::Future;

use super::{ServiceError, ServiceResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhotoLifecycle {
    Imported,
    Deleted,
    Other,
}

pub trait PhotoRepository: Clone + Send + Sync + 'static {
    fn lifecycle(
        &self,
        photo_id: i64,
    ) -> impl Future<Output = ServiceResult<Option<PhotoLifecycle>>> + Send;
    fn active_count(&self) -> impl Future<Output = ServiceResult<i64>> + Send;
}

#[derive(Clone)]
pub struct SqlitePhotoRepository {
    pool: SqlitePool,
}

impl SqlitePhotoRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

impl PhotoRepository for SqlitePhotoRepository {
    async fn lifecycle(&self, photo_id: i64) -> ServiceResult<Option<PhotoLifecycle>> {
        let status: Option<String> =
            sqlx::query_scalar("SELECT import_status FROM photos WHERE id = ?")
                .bind(photo_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(crate::error::AppError::from)
                .map_err(ServiceError::from)?;
        Ok(status.map(|status| match status.as_str() {
            "imported" => PhotoLifecycle::Imported,
            "deleted" => PhotoLifecycle::Deleted,
            _ => PhotoLifecycle::Other,
        }))
    }

    async fn active_count(&self) -> ServiceResult<i64> {
        sqlx::query_scalar("SELECT COUNT(*) FROM photos WHERE import_status = 'imported'")
            .fetch_one(&self.pool)
            .await
            .map_err(crate::error::AppError::from)
            .map_err(ServiceError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn repository() -> SqlitePhotoRepository {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO photos (id, path, sha256, format, import_status) VALUES \
             (1, '/active.jpg', 'active', 'jpeg', 'imported'), \
             (2, '/deleted.jpg', 'deleted', 'jpeg', 'deleted')",
        )
        .execute(&pool)
        .await
        .unwrap();
        SqlitePhotoRepository::new(pool)
    }

    #[tokio::test]
    async fn lifecycle_distinguishes_active_deleted_and_missing() {
        let repository = repository().await;
        assert_eq!(
            repository.lifecycle(1).await.unwrap(),
            Some(PhotoLifecycle::Imported)
        );
        assert_eq!(
            repository.lifecycle(2).await.unwrap(),
            Some(PhotoLifecycle::Deleted)
        );
        assert_eq!(repository.lifecycle(3).await.unwrap(), None);
    }

    #[tokio::test]
    async fn active_count_uses_lifecycle_filter_instead_of_cached_stats() {
        let repository = repository().await;
        assert_eq!(repository.active_count().await.unwrap(), 1);
    }
}
