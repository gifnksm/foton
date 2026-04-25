use std::{fs::File, io};

use fd_lock::{RwLock, RwLockWriteGuard};

use crate::util::{app_dirs::AppDirs, path::AbsolutePath};

#[derive(Debug)]
pub(crate) struct DbLockFile {
    path: AbsolutePath,
    lock: RwLock<File>,
}

#[derive(Debug)]
pub(crate) struct DbLockFileGuard<'a> {
    _guard: RwLockWriteGuard<'a, File>,
}

#[derive(Debug, derive_more::Display, derive_more::Error, derive_more::IsVariant)]
pub(crate) enum DbLockFileError {
    #[display("failed to open database lock file: {path}", path = path.display())]
    Open {
        path: AbsolutePath,
        #[error(source)]
        source: io::Error,
    },
    #[display("failed to acquire database lock: {path}", path = path.display())]
    Acquire {
        path: AbsolutePath,
        #[error(source)]
        source: io::Error,
    },
    #[display("database is already locked: {path}", path = path.display())]
    AlreadyLocked {
        path: AbsolutePath,
        #[error(source)]
        source: io::Error,
    },
}

impl DbLockFile {
    pub(crate) fn open(app_dirs: &AppDirs) -> Result<Self, DbLockFileError> {
        let path = app_dirs.db_lock_file();
        let file = File::options()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .map_err(|source| {
                let path = path.clone();
                DbLockFileError::Open { path, source }
            })?;

        Ok(Self {
            path,
            lock: RwLock::new(file),
        })
    }

    pub(crate) fn try_acquire(&mut self) -> Result<DbLockFileGuard<'_>, DbLockFileError> {
        match self.lock.try_write() {
            Ok(guard) => Ok(DbLockFileGuard { _guard: guard }),
            Err(source) => {
                let path = self.path.clone();
                if source.kind() == io::ErrorKind::WouldBlock {
                    Err(DbLockFileError::AlreadyLocked { path, source })
                } else {
                    Err(DbLockFileError::Acquire { path, source })
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn make_app_dirs() -> (TempDir, AppDirs) {
        let tempdir = tempfile::tempdir().unwrap();
        let data_local_dir = AbsolutePath::new(tempdir.path()).unwrap();
        let app_dirs = AppDirs::new_for_test(data_local_dir);
        (tempdir, app_dirs)
    }

    #[test]
    fn try_acquire_returns_already_locked_when_lock_is_already_held() {
        let (_tempdir, app_dirs) = make_app_dirs();
        let mut first = DbLockFile::open(&app_dirs).unwrap();
        let mut second = DbLockFile::open(&app_dirs).unwrap();

        let _first_guard = first.try_acquire().unwrap();
        let err = second.try_acquire().unwrap_err();

        assert!(matches!(err, DbLockFileError::AlreadyLocked { .. }));
    }

    #[test]
    fn try_acquire_succeeds_after_previous_guard_is_dropped() {
        let (_tempdir, app_dirs) = make_app_dirs();
        let mut first = DbLockFile::open(&app_dirs).unwrap();
        let mut second = DbLockFile::open(&app_dirs).unwrap();

        {
            let _first_guard = first.try_acquire().unwrap();
            let err = second.try_acquire().unwrap_err();
            assert!(matches!(err, DbLockFileError::AlreadyLocked { .. }));
        }

        let _second_guard = second.try_acquire().unwrap();
    }
}
