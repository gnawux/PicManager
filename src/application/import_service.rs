use std::path::PathBuf;

use crate::importer::{BatchResult, SharedImportProgress};

use super::{
    Application, AuthorizationScope, RequestContext, ServiceError, ServiceErrorCode, ServiceResult,
};

#[derive(Debug, Clone)]
pub struct ImportCommand {
    pub source_dir: PathBuf,
    pub copy_only: bool,
    pub batch_size: Option<usize>,
    pub log_path: Option<PathBuf>,
    pub dry_run: bool,
}

impl ImportCommand {
    pub fn directory(source_dir: impl Into<PathBuf>, copy_only: bool) -> Self {
        Self {
            source_dir: source_dir.into(),
            copy_only,
            batch_size: None,
            log_path: None,
            dry_run: false,
        }
    }
}

#[derive(Clone)]
pub struct ImportService {
    application: Application,
}

impl ImportService {
    pub(crate) fn new(application: Application) -> Self {
        Self { application }
    }

    pub async fn execute(
        &self,
        context: &RequestContext,
        command: ImportCommand,
        progress: SharedImportProgress,
    ) -> ServiceResult<BatchResult> {
        self.authorize(context)?;
        if !command.source_dir.is_dir() {
            return Err(ServiceError::new(
                ServiceErrorCode::InvalidInput,
                "Import source must be an existing directory",
            ));
        }
        if command.batch_size == Some(0) {
            return Err(ServiceError::new(
                ServiceErrorCode::InvalidInput,
                "Import batch size must be greater than zero",
            ));
        }

        crate::importer::import_dir_batch(
            self.application.pool(),
            &command.source_dir,
            &self.application.config().library_path,
            command.copy_only,
            command.batch_size,
            command.log_path.as_deref(),
            command.dry_run,
            progress,
        )
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
        if !matches!(
            context.authorization,
            AuthorizationScope::LocalTrusted | AuthorizationScope::InternalWorker
        ) {
            return Err(ServiceError::new(
                ServiceErrorCode::Unauthorized,
                "Caller is not authorized to import into this library",
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

    async fn application(library_path: PathBuf) -> Application {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let mut config = Config::default();
        config.library_path = library_path.clone();
        config.db_path = library_path.join("picmanager.db");
        config.thumb_cache_dir = library_path.join(".thumbs");
        Application::new(pool, config)
    }

    #[tokio::test]
    async fn dry_run_scans_without_writing_catalog_or_media() {
        let source = tempfile::tempdir().unwrap();
        let library = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("not-a-photo.txt"), b"ignored").unwrap();
        let app = application(library.path().to_path_buf()).await;
        let context = app.request_context(CallerKind::Cli);
        let mut command = ImportCommand::directory(source.path(), true);
        command.dry_run = true;

        let result = app
            .imports()
            .execute(&context, command, SharedImportProgress::default())
            .await
            .unwrap();
        let photo_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM photos")
            .fetch_one(app.pool())
            .await
            .unwrap();
        assert_eq!(result.total_files, 0);
        assert_eq!(photo_count, 0);
        assert!(std::fs::read_dir(library.path()).unwrap().next().is_none());
    }

    #[tokio::test]
    async fn rejects_missing_sources_and_zero_batch_sizes() {
        let library = tempfile::tempdir().unwrap();
        let app = application(library.path().to_path_buf()).await;
        let context = app.request_context(CallerKind::LocalWeb);
        let missing = ImportCommand::directory(library.path().join("missing"), true);
        let error = app
            .imports()
            .execute(&context, missing, SharedImportProgress::default())
            .await
            .unwrap_err();
        assert_eq!(error.code, ServiceErrorCode::InvalidInput);

        let mut zero = ImportCommand::directory(library.path(), true);
        zero.batch_size = Some(0);
        let error = app
            .imports()
            .execute(&context, zero, SharedImportProgress::default())
            .await
            .unwrap_err();
        assert_eq!(error.code, ServiceErrorCode::InvalidInput);
    }
}
