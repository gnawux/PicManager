use picmanager::sync::{
    DiscoveryBatch, DiscoveryItem, claim_next_item, get_job, mark_item_succeeded,
    persist_discovery, recover_expired_leases,
};
use sqlx::sqlite::SqlitePoolOptions;

#[tokio::test]
async fn interrupted_worker_resumes_from_durable_item_after_database_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let database_url = format!("sqlite://{}?mode=rwc", temp.path().join("restart.db").display());
    let pool = SqlitePoolOptions::new().max_connections(2)
        .connect(&database_url).await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    let job = persist_discovery(&pool, &DiscoveryBatch {
        kind: "apple_incremental",
        provider: "apple_photos",
        scope_key: "system",
        checkpoint_before: None,
        checkpoint_after: Some(b"token-after-restart"),
        items: vec![DiscoveryItem {
            source_id: None,
            external_id: "apple-asset-1",
            operation: "download",
            payload_json: Some(r#"{"filename":"IMG_0001.HEIC"}"#),
            max_attempts: 3,
        }],
    }).await.unwrap();
    let leased = claim_next_item(&pool, "worker-before-crash", 300)
        .await.unwrap().unwrap();
    sqlx::query("UPDATE sync_items SET lease_expires_at = datetime('now', '-1 second') WHERE id = ?")
        .bind(leased.id).execute(&pool).await.unwrap();
    pool.close().await;

    let reopened = SqlitePoolOptions::new().max_connections(2)
        .connect(&database_url).await.unwrap();
    assert_eq!(recover_expired_leases(&reopened).await.unwrap(), 1);
    let resumed = claim_next_item(&reopened, "worker-after-restart", 60)
        .await.unwrap().unwrap();
    assert_eq!(resumed.id, leased.id);
    assert_eq!(resumed.attempt_count, 2);
    assert_eq!(resumed.payload_json, leased.payload_json);
    mark_item_succeeded(&reopened, resumed.id, "worker-after-restart")
        .await.unwrap();

    let detail = get_job(&reopened, job.id).await.unwrap();
    assert_eq!(detail.job.status, "completed");
    assert_eq!(detail.job.completed_items, 1);
    let token: Vec<u8> = sqlx::query_scalar(
        "SELECT token FROM provider_checkpoints WHERE provider = 'apple_photos' AND scope_key = 'system'",
    ).fetch_one(&reopened).await.unwrap();
    assert_eq!(token, b"token-after-restart");
}
