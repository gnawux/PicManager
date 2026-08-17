use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use thiserror::Error;

pub const TOKEN_STORE_FILENAME: &str = "garmin_tokens.json";

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct GarminTokens {
    pub di_token: String,
    pub di_refresh_token: Option<String>,
    pub di_client_id: Option<String>,
}

impl GarminTokens {
    pub fn validate(self) -> Result<Self, TokenStoreError> {
        if self.di_token.trim().is_empty() {
            return Err(TokenStoreError::Invalid);
        }
        if self
            .di_refresh_token
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
            || self
                .di_client_id
                .as_deref()
                .is_some_and(|value| value.trim().is_empty())
        {
            return Err(TokenStoreError::Invalid);
        }
        Ok(self)
    }
}

#[derive(Debug, Error)]
pub enum TokenStoreError {
    #[error("Garmin token store contains invalid data")]
    Invalid,
    #[error("Garmin token store path must not contain symlinks")]
    Symlink,
    #[error("Garmin token store is unavailable")]
    Io(#[from] std::io::Error),
    #[error("Garmin token store is malformed")]
    Json(#[from] serde_json::Error),
}

#[derive(Clone, Debug)]
pub struct TokenStore {
    path: PathBuf,
}

impl TokenStore {
    pub fn in_directory(directory: impl AsRef<Path>) -> Self {
        Self {
            path: directory.as_ref().join(TOKEN_STORE_FILENAME),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn exists(&self) -> Result<bool, TokenStoreError> {
        reject_symlinks(&self.path)?;
        match fs::symlink_metadata(&self.path) {
            Ok(metadata) => Ok(metadata.file_type().is_file()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    pub fn load(&self) -> Result<GarminTokens, TokenStoreError> {
        reject_symlinks(&self.path)?;
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let file = options.open(&self.path)?;
        let mut payload = Vec::new();
        file.take(1024 * 1024 + 1).read_to_end(&mut payload)?;
        if payload.len() > 1024 * 1024 {
            return Err(TokenStoreError::Invalid);
        }
        serde_json::from_slice::<GarminTokens>(&payload)?.validate()
    }

    pub fn save(&self, tokens: &GarminTokens) -> Result<(), TokenStoreError> {
        tokens.clone().validate()?;
        reject_symlinks(&self.path)?;
        let parent = self.path.parent().ok_or(TokenStoreError::Invalid)?;
        fs::create_dir_all(parent)?;
        reject_symlinks(&self.path)?;
        set_directory_permissions(parent)?;

        let mut temporary = NamedTempFile::new_in(parent)?;
        set_file_permissions(temporary.as_file())?;
        serde_json::to_writer(temporary.as_file_mut(), tokens)?;
        temporary.as_file_mut().write_all(b"\n")?;
        temporary.as_file_mut().flush()?;
        temporary.as_file().sync_all()?;
        reject_symlinks(&self.path)?;
        temporary.persist(&self.path).map_err(|error| error.error)?;
        let persisted = File::open(&self.path)?;
        set_file_permissions(&persisted)?;
        persisted.sync_all()?;
        File::open(parent)?.sync_all()?;
        Ok(())
    }
}

fn reject_symlinks(path: &Path) -> Result<(), TokenStoreError> {
    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(TokenStoreError::Invalid);
    }
    // Reject the provider-owned token directory and file. System ancestors can
    // legitimately contain links on macOS (/var -> /private/var), so callers
    // must pass the configured absolute state root rather than an untrusted path.
    for current in [path.parent(), Some(path)].into_iter().flatten() {
        match fs::symlink_metadata(current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(TokenStoreError::Symlink);
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

#[cfg(unix)]
fn set_directory_permissions(path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn set_directory_permissions(_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
}

#[cfg(unix)]
fn set_file_permissions(file: &File) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_file_permissions(_file: &File) -> Result<(), std::io::Error> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens() -> GarminTokens {
        GarminTokens {
            di_token: "header.payload.signature".to_owned(),
            di_refresh_token: Some("refresh-value".to_owned()),
            di_client_id: Some("native-client".to_owned()),
        }
    }

    #[test]
    fn reads_literal_python_token_schema_and_ignores_future_fields() {
        let directory = tempfile::tempdir().unwrap();
        let store = TokenStore::in_directory(directory.path());
        fs::write(
            store.path(),
            r#"{"di_token":"header.payload.signature","di_refresh_token":"refresh-value","di_client_id":"native-client","future_field":true}"#,
        )
        .unwrap();

        assert_eq!(store.load().unwrap(), tokens());
    }

    #[test]
    fn saves_atomically_with_owner_only_permissions() {
        let directory = tempfile::tempdir().unwrap();
        let token_directory = directory.path().join("tokens");
        let store = TokenStore::in_directory(&token_directory);
        store.save(&tokens()).unwrap();

        assert_eq!(store.load().unwrap(), tokens());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&token_directory).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(store.path()).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_store_and_parent() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let file_link = directory.path().join(TOKEN_STORE_FILENAME);
        symlink(outside.path().join("tokens.json"), &file_link).unwrap();
        assert!(matches!(
            TokenStore::in_directory(directory.path()).save(&tokens()),
            Err(TokenStoreError::Symlink)
        ));

        let parent_link = directory.path().join("linked");
        symlink(outside.path(), &parent_link).unwrap();
        assert!(matches!(
            TokenStore::in_directory(parent_link).load(),
            Err(TokenStoreError::Symlink)
        ));
    }

    #[test]
    fn rejects_missing_or_empty_access_token() {
        let directory = tempfile::tempdir().unwrap();
        let store = TokenStore::in_directory(directory.path());
        fs::write(store.path(), r#"{"di_token":""}"#).unwrap();
        assert!(matches!(store.load(), Err(TokenStoreError::Invalid)));
    }
}
