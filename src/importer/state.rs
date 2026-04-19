use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::path::Path;
use std::io::Read;
use crate::error::Result;

#[derive(Debug, PartialEq)]
pub enum ImportDecision {
    New,
    AlreadyImported,
}

pub fn compute_sha256(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Return New if this SHA has never been imported, AlreadyImported otherwise.
pub async fn decide(pool: &SqlitePool, sha256: &str) -> Result<ImportDecision> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM photos WHERE sha256 = ?)",
    )
    .bind(sha256)
    .fetch_one(pool)
    .await?;

    if exists {
        Ok(ImportDecision::AlreadyImported)
    } else {
        Ok(ImportDecision::New)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;
    use std::io::Write;
    use tempfile::NamedTempFile;

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    #[test]
    fn sha256_is_deterministic() {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(b"hello world").unwrap();
        let h1 = compute_sha256(f.path()).unwrap();
        let h2 = compute_sha256(f.path()).unwrap();
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn sha256_differs_for_different_content() {
        let mut f1 = NamedTempFile::new().unwrap();
        let mut f2 = NamedTempFile::new().unwrap();
        f1.write_all(b"aaa").unwrap();
        f2.write_all(b"bbb").unwrap();
        assert_ne!(
            compute_sha256(f1.path()).unwrap(),
            compute_sha256(f2.path()).unwrap()
        );
    }

    #[tokio::test]
    async fn new_sha_returns_new() {
        let pool = test_pool().await;
        let decision = decide(&pool, "deadbeef").await.unwrap();
        assert_eq!(decision, ImportDecision::New);
    }

    #[tokio::test]
    async fn known_sha_returns_already_imported() {
        let pool = test_pool().await;
        sqlx::query("INSERT INTO photos (path, sha256, format) VALUES (?, ?, ?)")
            .bind("/lib/2024-06-15/a.jpg")
            .bind("abc123")
            .bind("jpeg")
            .execute(&pool)
            .await
            .unwrap();

        let decision = decide(&pool, "abc123").await.unwrap();
        assert_eq!(decision, ImportDecision::AlreadyImported);
    }
}
