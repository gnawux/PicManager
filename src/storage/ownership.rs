use std::{
    fs::{File, OpenOptions, TryLockError},
    io::Write,
    path::{Path, PathBuf},
};

pub struct LibraryServiceOwnership {
    #[allow(dead_code)]
    file: File,
    pub lock_path: PathBuf,
}

impl LibraryServiceOwnership {
    pub fn acquire(library_path: &Path) -> anyhow::Result<Self> {
        std::fs::create_dir_all(library_path)?;
        let lock_path = library_path.join(".picmanager-service.lock");
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)?;
        file.try_lock().map_err(|error| match error {
            TryLockError::WouldBlock => anyhow::anyhow!(
                "PicManager library is already owned by another service: {}",
                library_path.display()
            ),
            TryLockError::Error(error) => error.into(),
        })?;
        file.set_len(0)?;
        writeln!(file, "pid={}", std::process::id())?;
        file.sync_data()?;
        Ok(Self { file, lock_path })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prevents_two_services_from_owning_one_library() {
        let library = tempfile::tempdir().unwrap();
        let first = LibraryServiceOwnership::acquire(library.path()).unwrap();
        let second = LibraryServiceOwnership::acquire(library.path());
        assert!(second.is_err());
        drop(first);
        assert!(LibraryServiceOwnership::acquire(library.path()).is_ok());
    }
}
