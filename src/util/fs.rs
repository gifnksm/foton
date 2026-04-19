use std::{
    fs, io,
    path::{Path, PathBuf},
};

use crate::util::error::IgnoreNotFound as _;

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum FsError {
    #[display("failed to remove directory: {path}", path = path.display())]
    RemoveDirectory {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },
    #[display("failed to create directory: {path}", path = path.display())]
    CreateDirectory {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },
    #[display("failed to remove file: {path}", path = path.display())]
    RemoveFile {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },
}

pub(crate) fn remove_dir_all_if_exists<P>(path: P) -> Result<(), FsError>
where
    P: AsRef<Path>,
{
    let path = path.as_ref();
    fs::remove_dir_all(path)
        .ignore_not_found()
        .map_err(|source| {
            let path = path.to_owned();
            FsError::RemoveDirectory { path, source }
        })?;
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
        return Err(FsError::RemoveDirectory {
            path: path.to_owned(),
            source: err,
        });
    }
    Ok(RemoveDirIfEmptyResult::RemovedOrNotPresent)
}

pub(crate) fn create_dir_all<P>(path: P) -> Result<(), FsError>
where
    P: AsRef<Path>,
{
    let path = path.as_ref();
    fs::create_dir_all(path).map_err(|source| {
        let path = path.to_owned();
        FsError::CreateDirectory { path, source }
    })?;
    Ok(())
}

pub(crate) fn create_dir<P>(path: P) -> Result<(), FsError>
where
    P: AsRef<Path>,
{
    let path = path.as_ref();
    fs::create_dir(path).map_err(|source| {
        let path = path.to_owned();
        FsError::CreateDirectory { path, source }
    })?;
    Ok(())
}

pub(crate) fn remove_file<P>(path: P) -> Result<(), FsError>
where
    P: AsRef<Path>,
{
    let path = path.as_ref();
    fs::remove_file(path).map_err(|source| {
        let path = path.to_owned();
        FsError::RemoveFile { path, source }
    })?;
    Ok(())
}
