use sqlx::{SqlitePool, sqlite::SqliteConnectOptions};
use std::str::FromStr;
use crate::error::Result;

pub async fn connect(db_url: &str) -> Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str(db_url)?
        .create_if_missing(true)
        .foreign_keys(true);

    let pool = SqlitePool::connect_with(opts).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn test_pool() -> SqlitePool {
        // 单连接共享内存库，连接关闭前数据持久
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn connect_creates_db_and_runs_migrations() {
        let pool = test_pool().await;
        // migrations が通れば photos テーブルが存在する
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM photos")
            .fetch_one(&pool)
            .await
            .expect("photos table should exist");
        assert_eq!(row.0, 0);
    }

    #[tokio::test]
    async fn insert_and_query_photo() {
        let pool = test_pool().await;

        sqlx::query(
            "INSERT INTO photos (path, sha256, format) VALUES (?, ?, ?)",
        )
        .bind("/tmp/test.jpg")
        .bind("abc123")
        .bind("jpeg")
        .execute(&pool)
        .await
        .unwrap();

        let row: (String, String) =
            sqlx::query_as("SELECT path, format FROM photos WHERE sha256 = ?")
                .bind("abc123")
                .fetch_one(&pool)
                .await
                .unwrap();

        assert_eq!(row.0, "/tmp/test.jpg");
        assert_eq!(row.1, "jpeg");
    }

    #[tokio::test]
    async fn foreign_keys_enforced() {
        let pool = test_pool().await;
        // photo_albums は photos が存在しないと挿入できない
        let result = sqlx::query(
            "INSERT INTO photo_albums (photo_id, album_id) VALUES (999, 999)",
        )
        .execute(&pool)
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn all_tables_exist() {
        let pool = test_pool().await;
        for table in &["photos", "albums", "photo_albums", "dedup_groups", "dedup_members", "import_sessions"] {
            let row: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?",
            )
            .bind(table)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(row.0, 1, "table {table} should exist");
        }
    }
}
