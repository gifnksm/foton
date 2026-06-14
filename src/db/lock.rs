use std::{
    fs::{File, TryLockError},
    io,
    path::PathBuf,
};

use snafu::{IntoError as _, ResultExt as _, Snafu};

use crate::util::{app_dirs::AppDirs, macros::concat_line, path::AbsolutePath};

#[derive(Debug)]
pub(crate) struct DbLockFile {
    path: AbsolutePath,
    lock_file: File,
}

#[derive(Debug)]
pub(crate) struct DbLockFileGuard<'a> {
    lock: &'a DbLockFile,
}

#[derive(Debug, Snafu, derive_more::IsVariant)]
pub(crate) enum DbLockFileError {
    #[snafu(display("failed to open database lock file: {path}", path = path.display()))]
    Open { path: PathBuf, source: io::Error },
    #[snafu(display("failed to acquire database lock: {path}", path = path.display()))]
    Lock { path: PathBuf, source: io::Error },
    #[snafu(display(
        concat_line!(
            "database is already locked: {path}",
            "another operation is already in progress",
        ),
        path = path.display()
    ))]
    AlreadyLocked { path: PathBuf },
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
            .context(OpenSnafu { path: &path })?;

        Ok(Self {
            path,
            lock_file: file,
        })
    }

    pub(crate) fn try_lock(&mut self) -> Result<DbLockFileGuard<'_>, DbLockFileError> {
        self.lock_file.try_lock().map_err(|source| match source {
            TryLockError::Error(source) => LockSnafu { path: &self.path }.into_error(source),
            TryLockError::WouldBlock => AlreadyLockedSnafu { path: &self.path }.build(),
        })?;
        Ok(DbLockFileGuard { lock: self })
    }
}

impl Drop for DbLockFileGuard<'_> {
    fn drop(&mut self) {
        // `Drop` cannot propagate errors, so unlocking is best-effort here.
        // This guard is only created after a successful lock acquisition, and if
        // unlocking still fails there is no meaningful recovery available.
        let _ = self.lock.lock_file.unlock();
    }
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::*;
    use crate::util::testing;

    #[test]
    fn try_lock_returns_already_locked_when_lock_is_already_held() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut first = DbLockFile::open(&app_dirs).unwrap();
        let mut second = DbLockFile::open(&app_dirs).unwrap();

        let _first_guard = first.try_lock().unwrap();
        let err = second.try_lock().unwrap_err();

        assert_matches!(err, DbLockFileError::AlreadyLocked { .. });
    }

    #[test]
    fn try_lock_succeeds_after_previous_guard_is_dropped() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        let mut first = DbLockFile::open(&app_dirs).unwrap();
        let mut second = DbLockFile::open(&app_dirs).unwrap();

        {
            let _first_guard = first.try_lock().unwrap();
            let err = second.try_lock().unwrap_err();
            assert_matches!(err, DbLockFileError::AlreadyLocked { .. });
        }

        let _second_guard = second.try_lock().unwrap();
    }
}
