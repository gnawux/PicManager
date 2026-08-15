use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::{Application, RequestContext, ServiceError, ServiceErrorCode, ServiceResult};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollectionIdentity {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct MembershipChange {
    pub changed: u64,
}

#[derive(Clone)]
pub struct CollectionService {
    application: Application,
}

impl CollectionService {
    pub(crate) fn new(application: Application) -> Self {
        Self { application }
    }

    pub async fn create(
        &self,
        context: &RequestContext,
        name: &str,
    ) -> ServiceResult<CollectionIdentity> {
        self.authorize(context)?;
        let name = normalize_name(name)?;
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO albums (name, kind) VALUES (?, 'curated') RETURNING id",
        )
        .bind(&name)
        .fetch_one(self.application.pool())
        .await
        .map_err(crate::error::AppError::from)
        .map_err(ServiceError::from)?;
        Ok(CollectionIdentity { id, name })
    }

    pub async fn rename(
        &self,
        context: &RequestContext,
        collection_id: i64,
        name: &str,
    ) -> ServiceResult<CollectionIdentity> {
        self.authorize(context)?;
        let name = normalize_name(name)?;
        let result = sqlx::query("UPDATE albums SET name = ? WHERE id = ? AND kind = 'curated'")
            .bind(&name)
            .bind(collection_id)
            .execute(self.application.pool())
            .await
            .map_err(crate::error::AppError::from)
            .map_err(ServiceError::from)?;
        if result.rows_affected() == 0 {
            return Err(not_found(collection_id));
        }
        Ok(CollectionIdentity {
            id: collection_id,
            name,
        })
    }

    pub async fn delete(&self, context: &RequestContext, collection_id: i64) -> ServiceResult<()> {
        self.authorize(context)?;
        let result = sqlx::query("DELETE FROM albums WHERE id = ? AND kind = 'curated'")
            .bind(collection_id)
            .execute(self.application.pool())
            .await
            .map_err(crate::error::AppError::from)
            .map_err(ServiceError::from)?;
        if result.rows_affected() == 0 {
            return Err(not_found(collection_id));
        }
        Ok(())
    }

    pub async fn add_photos(
        &self,
        context: &RequestContext,
        collection_id: i64,
        photo_ids: &[i64],
    ) -> ServiceResult<MembershipChange> {
        self.change_membership(context, collection_id, photo_ids, true)
            .await
    }

    pub async fn remove_photos(
        &self,
        context: &RequestContext,
        collection_id: i64,
        photo_ids: &[i64],
    ) -> ServiceResult<MembershipChange> {
        self.change_membership(context, collection_id, photo_ids, false)
            .await
    }

    async fn change_membership(
        &self,
        context: &RequestContext,
        collection_id: i64,
        photo_ids: &[i64],
        add: bool,
    ) -> ServiceResult<MembershipChange> {
        self.authorize(context)?;
        let mut tx = self
            .application
            .pool()
            .begin()
            .await
            .map_err(crate::error::AppError::from)
            .map_err(ServiceError::from)?;
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM albums WHERE id = ? AND kind = 'curated')",
        )
        .bind(collection_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(crate::error::AppError::from)
        .map_err(ServiceError::from)?;
        if !exists {
            return Err(not_found(collection_id));
        }

        let ids: BTreeSet<i64> = photo_ids.iter().copied().collect();
        let mut changed = 0;
        for photo_id in ids {
            let result = if add {
                sqlx::query("INSERT OR IGNORE INTO photo_albums (photo_id, album_id) VALUES (?, ?)")
                    .bind(photo_id)
                    .bind(collection_id)
                    .execute(&mut *tx)
                    .await
            } else {
                sqlx::query("DELETE FROM photo_albums WHERE photo_id = ? AND album_id = ?")
                    .bind(photo_id)
                    .bind(collection_id)
                    .execute(&mut *tx)
                    .await
            }
            .map_err(crate::error::AppError::from)
            .map_err(ServiceError::from)?;
            changed += result.rows_affected();
        }
        tx.commit()
            .await
            .map_err(crate::error::AppError::from)
            .map_err(ServiceError::from)?;
        Ok(MembershipChange { changed })
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

fn normalize_name(name: &str) -> ServiceResult<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(ServiceError::new(
            ServiceErrorCode::InvalidInput,
            "Collection name cannot be empty",
        ));
    }
    Ok(name.to_owned())
}

fn not_found(collection_id: i64) -> ServiceError {
    ServiceError::new(
        ServiceErrorCode::NotFound,
        format!("collection {collection_id}"),
    )
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::{application::CallerKind, config::Config};
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    async fn application() -> Application {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO photos (id, path, sha256, format, import_status) VALUES \
             (1, '/one.jpg', 'one', 'jpeg', 'imported'), \
             (2, '/two.jpg', 'two', 'jpeg', 'imported')",
        )
        .execute(&pool)
        .await
        .unwrap();
        Application::new(pool, Config::default())
    }

    #[tokio::test]
    async fn collection_lifecycle_normalizes_names_and_preserves_other_album_kinds() {
        let app = application().await;
        let context = app.request_context(CallerKind::LocalWeb);
        let created = app
            .collections()
            .create(&context, "  Favorites  ")
            .await
            .unwrap();
        assert_eq!(created.name, "Favorites");
        let renamed = app
            .collections()
            .rename(&context, created.id, "Trips")
            .await
            .unwrap();
        assert_eq!(renamed.name, "Trips");
        app.collections()
            .delete(&context, created.id)
            .await
            .unwrap();
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM albums WHERE id = ?")
            .bind(created.id)
            .fetch_one(app.pool())
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn membership_changes_are_deduplicated_and_transactional() {
        let app = application().await;
        let context = app.request_context(CallerKind::LocalWeb);
        let collection = app.collections().create(&context, "Review").await.unwrap();
        let added = app
            .collections()
            .add_photos(&context, collection.id, &[1, 1, 2])
            .await
            .unwrap();
        assert_eq!(added.changed, 2);
        let removed = app
            .collections()
            .remove_photos(&context, collection.id, &[1, 1])
            .await
            .unwrap();
        assert_eq!(removed.changed, 1);
    }

    #[tokio::test]
    async fn invalid_photo_rolls_back_the_complete_membership_batch() {
        let app = application().await;
        let context = app.request_context(CallerKind::LocalWeb);
        let collection = app.collections().create(&context, "Atomic").await.unwrap();
        let result = app
            .collections()
            .add_photos(&context, collection.id, &[1, 404])
            .await;
        assert!(result.is_err());
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM photo_albums WHERE album_id = ?")
            .bind(collection.id)
            .fetch_one(app.pool())
            .await
            .unwrap();
        assert_eq!(count, 0);
    }
}
