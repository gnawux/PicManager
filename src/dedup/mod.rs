pub mod candidate;
pub mod hash;

use serde::Serialize;
use sqlx::SqlitePool;
use crate::error::Result;

pub use candidate::{scan, scan_full};

#[derive(Debug, Serialize)]
pub struct DedupMember {
    pub photo_id: i64,
    pub path: String,
    pub taken_at: Option<String>,
    pub camera: Option<String>,
    pub keep: bool,
}

#[derive(Debug, Serialize)]
pub struct DedupGroup {
    pub group_id: i64,
    pub status: String,
    pub members: Vec<DedupMember>,
}

pub async fn list_groups(pool: &SqlitePool) -> Result<Vec<DedupGroup>> {
    let group_rows: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, status FROM dedup_groups WHERE status = 'pending' ORDER BY id")
            .fetch_all(pool)
            .await?;

    let mut groups = Vec::new();
    for (group_id, status) in group_rows {
        let members: Vec<DedupMember> = sqlx::query_as(
            "SELECT dm.photo_id, p.path, p.taken_at, p.camera, dm.keep
             FROM dedup_members dm JOIN photos p ON p.id = dm.photo_id
             WHERE dm.group_id = ? ORDER BY dm.photo_id",
        )
        .bind(group_id)
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|(photo_id, path, taken_at, camera, keep): (i64, String, Option<String>, Option<String>, bool)| {
            DedupMember { photo_id, path, taken_at, camera, keep }
        })
        .collect();

        groups.push(DedupGroup { group_id, status, members });
    }
    Ok(groups)
}

/// Mark `keep_ids` as kept, soft-delete the rest, resolve the group.
pub async fn resolve(pool: &SqlitePool, group_id: i64, keep_ids: &[i64]) -> Result<()> {
    // Verify group exists and is pending
    let row: Option<(String,)> =
        sqlx::query_as("SELECT status FROM dedup_groups WHERE id = ?")
            .bind(group_id)
            .fetch_optional(pool)
            .await?;
    match row {
        None => return Err(crate::error::AppError::NotFound(format!("dedup group {group_id}"))),
        Some((s,)) if s != "pending" => return Ok(()), // already resolved
        _ => {}
    }

    // Mark keep flags on members
    sqlx::query("UPDATE dedup_members SET keep = 0 WHERE group_id = ?")
        .bind(group_id)
        .execute(pool)
        .await?;
    for id in keep_ids {
        sqlx::query("UPDATE dedup_members SET keep = 1 WHERE group_id = ? AND photo_id = ?")
            .bind(group_id)
            .bind(id)
            .execute(pool)
            .await?;
    }

    // Soft-delete non-kept photos and decrement counter
    sqlx::query(
        "UPDATE photos SET import_status = 'deleted'
         WHERE id IN (
             SELECT photo_id FROM dedup_members WHERE group_id = ? AND keep = 0
         )",
    )
    .bind(group_id)
    .execute(pool)
    .await?;

    sqlx::query(
        "UPDATE photo_stats SET active_count = active_count - (
             SELECT COUNT(*) FROM dedup_members WHERE group_id = ? AND keep = 0
         ) WHERE id = 1",
    )
    .bind(group_id)
    .execute(pool)
    .await?;

    // Mark group resolved
    sqlx::query("UPDATE dedup_groups SET status = 'resolved' WHERE id = ?")
        .bind(group_id)
        .execute(pool)
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    async fn setup_group(pool: &SqlitePool) -> (i64, i64, i64) {
        let pa = sqlx::query(
            "INSERT INTO photos (path, sha256, format, phash, import_status) VALUES ('/a.jpg','a','jpeg','AAAA','imported')",
        )
        .execute(pool).await.unwrap().last_insert_rowid();

        let pb = sqlx::query(
            "INSERT INTO photos (path, sha256, format, phash, import_status) VALUES ('/b.jpg','b','jpeg','AAAA','imported')",
        )
        .execute(pool).await.unwrap().last_insert_rowid();

        let gid = sqlx::query("INSERT INTO dedup_groups (status) VALUES ('pending')")
            .execute(pool).await.unwrap().last_insert_rowid();

        sqlx::query("INSERT INTO dedup_members (group_id, photo_id) VALUES (?,?),(?,?)")
            .bind(gid).bind(pa).bind(gid).bind(pb)
            .execute(pool).await.unwrap();

        (gid, pa, pb)
    }

    #[tokio::test]
    async fn list_groups_empty() {
        let pool = test_pool().await;
        let groups = list_groups(&pool).await.unwrap();
        assert!(groups.is_empty());
    }

    #[tokio::test]
    async fn list_groups_returns_pending() {
        let pool = test_pool().await;
        setup_group(&pool).await;
        let groups = list_groups(&pool).await.unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].members.len(), 2);
    }

    #[tokio::test]
    async fn resolve_marks_non_kept_as_deleted() {
        let pool = test_pool().await;
        let (gid, pa, pb) = setup_group(&pool).await;

        resolve(&pool, gid, &[pa]).await.unwrap();

        let status_a: (String,) = sqlx::query_as("SELECT import_status FROM photos WHERE id=?")
            .bind(pa).fetch_one(&pool).await.unwrap();
        let status_b: (String,) = sqlx::query_as("SELECT import_status FROM photos WHERE id=?")
            .bind(pb).fetch_one(&pool).await.unwrap();

        assert_eq!(status_a.0, "imported");
        assert_eq!(status_b.0, "deleted");
    }

    #[tokio::test]
    async fn resolve_marks_group_resolved() {
        let pool = test_pool().await;
        let (gid, pa, _) = setup_group(&pool).await;
        resolve(&pool, gid, &[pa]).await.unwrap();

        let status: (String,) = sqlx::query_as("SELECT status FROM dedup_groups WHERE id=?")
            .bind(gid).fetch_one(&pool).await.unwrap();
        assert_eq!(status.0, "resolved");
    }

    #[tokio::test]
    async fn resolved_group_not_in_list() {
        let pool = test_pool().await;
        let (gid, pa, _) = setup_group(&pool).await;
        resolve(&pool, gid, &[pa]).await.unwrap();

        let groups = list_groups(&pool).await.unwrap();
        assert!(groups.is_empty());
    }

    #[tokio::test]
    async fn resolve_nonexistent_group_returns_error() {
        let pool = test_pool().await;
        let result = resolve(&pool, 999, &[1]).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn resolve_decrements_active_count() {
        let pool = test_pool().await;

        // Seed counter manually (no importer used in this test).
        sqlx::query("UPDATE photo_stats SET active_count = 2 WHERE id = 1")
            .execute(&pool)
            .await
            .unwrap();

        let (gid, pa, _pb) = setup_group(&pool).await;
        resolve(&pool, gid, &[pa]).await.unwrap();

        let count: (i64,) =
            sqlx::query_as("SELECT active_count FROM photo_stats WHERE id = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count.0, 1, "soft-deleting one photo should decrement by 1");
    }
}
