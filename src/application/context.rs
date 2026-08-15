use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use sqlx::SqlitePool;

use crate::config::Config;

static REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallerKind {
    Cli,
    LocalWeb,
    MacApp,
    InternalWorker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorizationScope {
    LocalTrusted,
    InternalWorker,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LibraryIdentity(Arc<str>);

impl LibraryIdentity {
    pub fn from_config(config: &Config) -> Self {
        Self(Arc::from(
            config.library_path.to_string_lossy().into_owned(),
        ))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestContext {
    pub caller: CallerKind,
    pub request_id: Arc<str>,
    pub library: LibraryIdentity,
    pub authorization: AuthorizationScope,
    pub idempotency_key: Option<Arc<str>>,
}

impl RequestContext {
    pub fn with_idempotency_key(mut self, key: impl Into<Arc<str>>) -> Self {
        self.idempotency_key = Some(key.into());
        self
    }
}

#[derive(Clone)]
pub struct Application {
    pool: SqlitePool,
    config: Arc<Config>,
    library: LibraryIdentity,
}

impl Application {
    pub fn new(pool: SqlitePool, config: Config) -> Self {
        let library = LibraryIdentity::from_config(&config);
        Self {
            pool,
            config: Arc::new(config),
            library,
        }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn library(&self) -> &LibraryIdentity {
        &self.library
    }

    pub fn request_context(&self, caller: CallerKind) -> RequestContext {
        let authorization = match caller {
            CallerKind::InternalWorker => AuthorizationScope::InternalWorker,
            _ => AuthorizationScope::LocalTrusted,
        };
        let sequence = REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        RequestContext {
            caller,
            request_id: Arc::from(format!("request-{sequence}")),
            library: self.library.clone(),
            authorization,
            idempotency_key: None,
        }
    }

    pub fn imports(&self) -> super::ImportService {
        super::ImportService::new(self.clone())
    }

    pub fn dedup(&self) -> super::DedupService {
        super::DedupService::new(self.clone())
    }

    pub fn metadata(&self) -> super::PhotoMetadataService {
        super::PhotoMetadataService::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn application() -> Application {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        Application::new(pool, Config::default())
    }

    #[tokio::test]
    async fn creates_unique_contexts_for_one_library() {
        let app = application().await;
        let first = app.request_context(CallerKind::Cli);
        let second = app.request_context(CallerKind::LocalWeb);

        assert_ne!(first.request_id, second.request_id);
        assert_eq!(first.library, second.library);
        assert_eq!(first.authorization, AuthorizationScope::LocalTrusted);
    }

    #[tokio::test]
    async fn worker_context_has_internal_scope_and_idempotency() {
        let app = application().await;
        let context = app
            .request_context(CallerKind::InternalWorker)
            .with_idempotency_key("thumbnail:42:r3");

        assert_eq!(context.authorization, AuthorizationScope::InternalWorker);
        assert_eq!(context.idempotency_key.as_deref(), Some("thumbnail:42:r3"));
    }
}
