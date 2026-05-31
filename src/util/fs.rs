use std::{
    fs, io,
    path::{Path, PathBuf},
};

use snafu::{IntoError as _, ResultExt as _, Snafu};

use crate::util::error::IgnoreNotFound as _;

#[derive(Debug, Snafu)]
pub(crate) enum FsError {
    #[snafu(display("failed to remove directory: {path}", path = path.display()))]
    RemoveDirectory { path: PathBuf, source: io::Error },
    #[snafu(display("failed to create directory: {path}", path = path.display()))]
    CreateDirectory { path: PathBuf, source: io::Error },
    #[snafu(display("failed to remove file: {path}", path = path.display()))]
    RemoveFile { path: PathBuf, source: io::Error },
}

pub(crate) fn remove_dir_all_if_exists<P>(path: P) -> Result<(), FsError>
where
    P: AsRef<Path>,
{
    let path = path.as_ref();
    fs::remove_dir_all(path)
        .ignore_not_found()
        .context(RemoveDirectorySnafu { path })?;
    Ok(())
}

pub(crate) fn remove_dir_if_exists<P>(path: P) -> Result<(), FsError>
where
    P: AsRef<Path>,
{
    let path = path.as_ref();
    fs::remove_dir(path)
        .ignore_not_found()
        .context(RemoveDirectorySnafu { path })?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, derive_more::IsVariant)]
pub(crate) enum RemoveDirIfEmptyResult {
    RemovedOrNotPresent,
    NotEmpty,
}

pub(crate) fn remove_dir_if_empty<P>(path: P) -> Result<RemoveDirIfEmptyResult, FsError>
where
    P: AsRef<Path>,
{
    let path = path.as_ref();
    if let Err(err) = fs::remove_dir(path) {
        if err.kind() == io::ErrorKind::NotFound {
            return Ok(RemoveDirIfEmptyResult::RemovedOrNotPresent);
        }
        if err.kind() == io::ErrorKind::DirectoryNotEmpty {
            return Ok(RemoveDirIfEmptyResult::NotEmpty);
        }
        return Err(RemoveDirectorySnafu { path }.into_error(err));
    }
    Ok(RemoveDirIfEmptyResult::RemovedOrNotPresent)
}

pub(crate) fn create_dir_all<P>(path: P) -> Result<(), FsError>
where
    P: AsRef<Path>,
{
    let path = path.as_ref();
    fs::create_dir_all(path).context(CreateDirectorySnafu { path })?;
    Ok(())
}

pub(crate) fn create_dir<P>(path: P) -> Result<(), FsError>
where
    P: AsRef<Path>,
{
    let path = path.as_ref();
    fs::create_dir(path).context(CreateDirectorySnafu { path })?;
    Ok(())
}

pub(crate) fn remove_file<P>(path: P) -> Result<(), FsError>
where
    P: AsRef<Path>,
{
    let path = path.as_ref();
    fs::remove_file(path).context(RemoveFileSnafu { path })?;
    Ok(())
}
