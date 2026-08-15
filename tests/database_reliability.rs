use std::time::Duration;

use picmanager::jobs::{
    HandlerFuture, Job, JobControl, JobHandler, NewJob, WorkerConfig, WorkerRegistry,
    WorkerRuntime, enqueue, get, lease_next,
};
use picmanager::storage::connect_with_settings;

struct RestartHandler;

impl JobHandler for RestartHandler {
    fn execute(&self, _job: Job, _control: JobControl) -> HandlerFuture {
        Box::pin(async { Ok(()) })
    }
}

#[tokio::test]
async fn busy_timeout_allows_a_waiting_writer_to_finish() {
    let directory = tempfile::tempdir().unwrap();
    let url = format!(
        "sqlite:{}",
        directory.path().join("contention.db").display()
    );
    let pool = connect_with_settings(&url, 3, Duration::from_secs(2))
        .await
        .unwrap();
    let mut owner = pool.acquire().await.unwrap();
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *owner)
        .await
        .unwrap();
    sqlx::query("INSERT INTO albums (name, kind) VALUES ('owner', 'manual')")
        .execute(&mut *owner)
        .await
        .unwrap();

    let waiting_pool = pool.clone();
    let waiting = tokio::spawn(async move {
        sqlx::query("INSERT INTO albums (name, kind) VALUES ('waiting', 'manual')")
            .execute(&waiting_pool)
            .await
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!waiting.is_finished());
    sqlx::query("COMMIT").execute(&mut *owner).await.unwrap();
    waiting.await.unwrap().unwrap();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM albums")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn expired_job_lease_recovers_after_database_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let url = format!("sqlite:{}", directory.path().join("restart.db").display());
    let pool = connect_with_settings(&url, 3, Duration::from_secs(2))
        .await
        .unwrap();
    let job = enqueue(&pool, &NewJob::new("restart_test", serde_json::json!({})))
        .await
        .unwrap()
        .job;
    lease_next(&pool, "dead-worker", 60).await.unwrap().unwrap();
    sqlx::query(
        "UPDATE application_jobs SET lease_expires_at = datetime('now', '-1 second') WHERE id = ?",
    )
    .bind(job.id)
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;

    let reopened = connect_with_settings(&url, 3, Duration::from_secs(2))
        .await
        .unwrap();
    let worker = WorkerRuntime::new(
        reopened.clone(),
        WorkerRegistry::new().register("restart_test", RestartHandler),
        WorkerConfig {
            concurrency: 1,
            poll_interval: Duration::from_millis(10),
            shutdown_timeout: Duration::from_secs(2),
            ..Default::default()
        },
        "restart-worker",
    )
    .start();
    let completed = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let current = get(&reopened, job.id).await.unwrap();
            if current.status == "succeeded" {
                break current;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(worker.shutdown().await);
    assert_eq!(completed.attempt_count, 2);
}
