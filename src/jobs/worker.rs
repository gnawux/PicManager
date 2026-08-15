use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc, time::Duration};

use sqlx::SqlitePool;
use tokio::{sync::watch, task::JoinHandle};

use crate::error::Result;

use super::{
    Job, JobFailure, JobLease, cancel_leased, cancellation_requested, complete, fail, lease_next,
    recover_expired, renew_lease, set_result, update_progress,
};

pub type HandlerFuture = Pin<Box<dyn Future<Output = std::result::Result<(), JobFailure>> + Send>>;

pub trait JobHandler: Send + Sync + 'static {
    fn execute(&self, job: Job, control: JobControl) -> HandlerFuture;
}

#[derive(Clone, Default)]
pub struct WorkerRegistry {
    handlers: HashMap<String, Arc<dyn JobHandler>>,
}

impl WorkerRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(mut self, kind: impl Into<String>, handler: impl JobHandler) -> Self {
        self.handlers.insert(kind.into(), Arc::new(handler));
        self
    }

    fn handler(&self, kind: &str) -> Option<Arc<dyn JobHandler>> {
        self.handlers.get(kind).cloned()
    }
}

#[derive(Debug, Clone)]
pub struct WorkerConfig {
    pub concurrency: usize,
    pub lease_duration: Duration,
    pub poll_interval: Duration,
    pub retry_delay: Duration,
    pub shutdown_timeout: Duration,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            concurrency: 2,
            lease_duration: Duration::from_secs(60),
            poll_interval: Duration::from_millis(250),
            retry_delay: Duration::from_secs(5),
            shutdown_timeout: Duration::from_secs(30),
        }
    }
}

#[derive(Clone)]
pub struct JobControl {
    pool: SqlitePool,
    lease: JobLease,
}

impl JobControl {
    pub async fn progress(
        &self,
        completed: i64,
        total: Option<i64>,
        stage: Option<&str>,
    ) -> Result<()> {
        update_progress(&self.pool, &self.lease, completed, total, stage).await
    }

    pub async fn cancellation_requested(&self) -> Result<bool> {
        cancellation_requested(&self.pool, &self.lease).await
    }

    pub async fn set_result(&self, result: &serde_json::Value) -> Result<()> {
        set_result(&self.pool, &self.lease, result).await
    }

    pub fn job_id(&self) -> i64 {
        self.lease.job.id
    }
}

pub struct WorkerRuntime {
    pool: SqlitePool,
    registry: Arc<WorkerRegistry>,
    config: WorkerConfig,
    worker_prefix: String,
}

impl WorkerRuntime {
    pub fn new(
        pool: SqlitePool,
        registry: WorkerRegistry,
        config: WorkerConfig,
        worker_prefix: impl Into<String>,
    ) -> Self {
        Self {
            pool,
            registry: Arc::new(registry),
            config,
            worker_prefix: worker_prefix.into(),
        }
    }

    pub fn start(self) -> WorkerHandle {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let timeout = self.config.shutdown_timeout;
        let join = tokio::spawn(self.run(shutdown_rx));
        WorkerHandle {
            shutdown_tx,
            join: Some(join),
            timeout,
        }
    }

    async fn run(self, shutdown: watch::Receiver<bool>) {
        if let Err(error) = recover_expired(&self.pool).await {
            tracing::error!("failed to recover expired application jobs: {error}");
        }
        let concurrency = self.config.concurrency.clamp(1, 64);
        let mut workers = Vec::with_capacity(concurrency);
        for slot in 0..concurrency {
            workers.push(tokio::spawn(worker_loop(
                self.pool.clone(),
                self.registry.clone(),
                self.config.clone(),
                format!("{}-{slot}", self.worker_prefix),
                shutdown.clone(),
            )));
        }
        for worker in workers {
            let _ = worker.await;
        }
    }
}

pub struct WorkerHandle {
    shutdown_tx: watch::Sender<bool>,
    join: Option<JoinHandle<()>>,
    timeout: Duration,
}

impl WorkerHandle {
    pub async fn shutdown(mut self) -> bool {
        let _ = self.shutdown_tx.send(true);
        let Some(mut join) = self.join.take() else {
            return true;
        };
        match tokio::time::timeout(self.timeout, &mut join).await {
            Ok(_) => true,
            Err(_) => {
                join.abort();
                false
            }
        }
    }
}

impl Drop for WorkerHandle {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(true);
    }
}

async fn worker_loop(
    pool: SqlitePool,
    registry: Arc<WorkerRegistry>,
    config: WorkerConfig,
    worker_id: String,
    mut shutdown: watch::Receiver<bool>,
) {
    loop {
        if *shutdown.borrow() {
            return;
        }
        match lease_next(&pool, &worker_id, config.lease_duration.as_secs()).await {
            Ok(Some(lease)) => execute_leased(&pool, &registry, &config, lease).await,
            Ok(None) => {
                tokio::select! {
                    _ = tokio::time::sleep(config.poll_interval) => {}
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() {
                            return;
                        }
                    }
                }
            }
            Err(error) => {
                tracing::error!(worker_id, "failed to lease application job: {error}");
                tokio::time::sleep(config.poll_interval).await;
            }
        }
    }
}

async fn execute_leased(
    pool: &SqlitePool,
    registry: &WorkerRegistry,
    config: &WorkerConfig,
    lease: JobLease,
) {
    let Some(handler) = registry.handler(&lease.job.kind) else {
        let failure = JobFailure::terminal(
            "unknown_job_kind",
            format!("No worker is registered for job kind {}", lease.job.kind),
        );
        let _ = fail(pool, &lease, &failure, 0).await;
        return;
    };
    let control = JobControl {
        pool: pool.clone(),
        lease: lease.clone(),
    };
    let execution = handler.execute(lease.job.clone(), control);
    tokio::pin!(execution);
    let heartbeat_period = (config.lease_duration / 3).max(Duration::from_millis(50));
    let mut heartbeat = tokio::time::interval(heartbeat_period);
    heartbeat.tick().await;

    let outcome = loop {
        tokio::select! {
            result = &mut execution => break Some(result),
            _ = heartbeat.tick() => {
                if let Err(error) = renew_lease(pool, &lease, config.lease_duration.as_secs()).await {
                    tracing::error!(job_id = lease.job.id, "lost job lease heartbeat: {error}");
                    break None;
                }
            }
        }
    };
    let Some(outcome) = outcome else {
        return;
    };
    let cancelled = cancellation_requested(pool, &lease).await.unwrap_or(false);
    match (outcome, cancelled) {
        (_, true) => {
            let _ = cancel_leased(pool, &lease).await;
        }
        (Ok(()), false) => {
            let _ = complete(pool, &lease).await;
        }
        (Err(failure), false) => {
            let _ = fail(pool, &lease, &failure, config.retry_delay.as_secs()).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::jobs::{NewJob, enqueue, list};
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    struct ConcurrencyHandler {
        active: Arc<AtomicUsize>,
        peak: Arc<AtomicUsize>,
    }

    impl JobHandler for ConcurrencyHandler {
        fn execute(&self, _job: Job, control: JobControl) -> HandlerFuture {
            let active = self.active.clone();
            let peak = self.peak.clone();
            Box::pin(async move {
                let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(current, Ordering::SeqCst);
                control.progress(1, Some(2), Some("working")).await.unwrap();
                tokio::time::sleep(Duration::from_millis(60)).await;
                control.progress(2, Some(2), Some("done")).await.unwrap();
                active.fetch_sub(1, Ordering::SeqCst);
                Ok(())
            })
        }
    }

    async fn pool() -> (SqlitePool, tempfile::TempDir) {
        let directory = tempfile::tempdir().unwrap();
        let options = SqliteConnectOptions::new()
            .filename(directory.path().join("jobs.db"))
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(6)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        (pool, directory)
    }

    #[tokio::test]
    async fn worker_respects_concurrency_and_completes_queued_jobs() {
        let (pool, _directory) = pool().await;
        for index in 0..6 {
            enqueue(
                &pool,
                &NewJob::new("test_work", serde_json::json!({"index": index})),
            )
            .await
            .unwrap();
        }
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let registry = WorkerRegistry::new().register(
            "test_work",
            ConcurrencyHandler {
                active: active.clone(),
                peak: peak.clone(),
            },
        );
        let handle = WorkerRuntime::new(
            pool.clone(),
            registry,
            WorkerConfig {
                concurrency: 2,
                lease_duration: Duration::from_secs(2),
                poll_interval: Duration::from_millis(10),
                shutdown_timeout: Duration::from_secs(2),
                ..Default::default()
            },
            "test-worker",
        )
        .start();

        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if list(&pool, Some("succeeded"), None, None, 20)
                    .await
                    .unwrap()
                    .len()
                    == 6
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert!(handle.shutdown().await);
        assert_eq!(active.load(Ordering::SeqCst), 0);
        assert_eq!(peak.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn unknown_job_kind_fails_visibly() {
        let (pool, _directory) = pool().await;
        enqueue(
            &pool,
            &NewJob::new("missing_handler", serde_json::json!({})),
        )
        .await
        .unwrap();
        let handle = WorkerRuntime::new(
            pool.clone(),
            WorkerRegistry::new(),
            WorkerConfig {
                concurrency: 1,
                poll_interval: Duration::from_millis(10),
                shutdown_timeout: Duration::from_secs(1),
                ..Default::default()
            },
            "test-worker",
        )
        .start();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if !list(&pool, Some("failed"), None, None, 10)
                    .await
                    .unwrap()
                    .is_empty()
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(handle.shutdown().await);
    }
}
