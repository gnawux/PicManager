use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::path::Path;
use std::io::Read;
use crate::error::Result;

#[derive(Debug, PartialEq)]
pub enum ImportDecision {
    New,
    AlreadyImported,
    Duplicate { existing_path: String },
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

pub async fn decide(pool: &SqlitePool, path: &Path, sha256: &str) -> Result<ImportDecision> {
    let path_str = path.to_string_lossy();

    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT path, import_status FROM photos WHERE sha256 = ? LIMIT 1",
    )
    .bind(sha256)
    .fetch_optional(pool)
    .await?;

    match row {
        None => Ok(ImportDecision::New),
        Some((existing_path, _)) if existing_path == path_str.as_ref() => {
            Ok(ImportDecision::AlreadyImported)
        }
        Some((existing_path, _)) => Ok(ImportDecision::Duplicate { existing_path }),
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
    async fn new_file_returns_new() {
        let pool = test_pool().await;
        let decision = decide(&pool, Path::new("/tmp/a.jpg"), "deadbeef").await.unwrap();
        assert_eq!(decision, ImportDecision::New);
    }

    #[tokio::test]
    async fn same_path_and_hash_returns_already_imported() {
        let pool = test_pool().await;
        sqlx::query("INSERT INTO photos (path, sha256, format) VALUES (?, ?, ?)")
            .bind("/tmp/a.jpg")
            .bind("abc123")
            .bind("jpeg")
            .execute(&pool)
            .await
            .unwrap();

        let decision = decide(&pool, Path::new("/tmp/a.jpg"), "abc123").await.unwrap();
        assert_eq!(decision, ImportDecision::AlreadyImported);
    }

    #[tokio::test]
    async fn same_hash_different_path_returns_duplicate() {
        let pool = test_pool().await;
        sqlx::query("INSERT INTO photos (path, sha256, format) VALUES (?, ?, ?)")
            .bind("/original/a.jpg")
            .bind("abc123")
            .bind("jpeg")
            .execute(&pool)
            .await
            .unwrap();

        let decision = decide(&pool, Path::new("/new/a.jpg"), "abc123").await.unwrap();
        assert!(matches!(decision, ImportDecision::Duplicate { .. }));
    }
}
