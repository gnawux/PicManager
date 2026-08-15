use crate::dedup::DedupGroup;

use super::{Application, RequestContext, ServiceError, ServiceErrorCode, ServiceResult};

#[derive(Clone)]
pub struct DedupService {
    application: Application,
}

impl DedupService {
    pub(crate) fn new(application: Application) -> Self {
        Self { application }
    }

    pub async fn scan(&self, context: &RequestContext, full: bool) -> ServiceResult<usize> {
        self.authorize(context)?;
        let result = if full {
            crate::dedup::scan_full(self.application.pool()).await
        } else {
            crate::dedup::scan(self.application.pool()).await
        };
        result.map_err(ServiceError::from)
    }

    pub async fn list(&self, context: &RequestContext) -> ServiceResult<Vec<DedupGroup>> {
        self.authorize(context)?;
        crate::dedup::list_groups(self.application.pool())
            .await
            .map_err(ServiceError::from)
    }

    pub async fn resolve(
        &self,
        context: &RequestContext,
        group_id: i64,
        keep_photo_ids: &[i64],
    ) -> ServiceResult<()> {
        self.authorize(context)?;
        if keep_photo_ids.is_empty() {
            return Err(ServiceError::new(
                ServiceErrorCode::InvalidInput,
                "At least one photo must be retained",
            ));
        }
        crate::dedup::resolve(self.application.pool(), group_id, keep_photo_ids)
            .await
            .map_err(ServiceError::from)
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
        Application::new(pool, Config::default())
    }

    #[tokio::test]
    async fn empty_catalog_scan_and_list_are_stable() {
        let app = application().await;
        let context = app.request_context(CallerKind::Cli);
        assert_eq!(app.dedup().scan(&context, false).await.unwrap(), 0);
        assert!(app.dedup().list(&context).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn resolution_requires_an_explicit_keep_selection() {
        let app = application().await;
        let context = app.request_context(CallerKind::LocalWeb);
        let error = app.dedup().resolve(&context, 1, &[]).await.unwrap_err();
        assert_eq!(error.code, ServiceErrorCode::InvalidInput);
    }
}
