use std::{fs, path::Path};

use color_eyre::eyre::{self, WrapErr as _};

use crate::util::error::IgnoreNotFound as _;

pub(crate) fn remove_dir_all_if_exists(path: &Path) -> eyre::Result<()> {
    fs::remove_dir_all(path)
        .ignore_not_found()
        .wrap_err_with(|| format!("failed to remove directory: {}", path.display()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, derive_more::IsVariant)]
pub(crate) enum RemoveDirIfEmptyResult {
    Removed,
    NotEmpty,
}

pub(crate) fn remove_dir_if_empty(path: &Path) -> eyre::Result<RemoveDirIfEmptyResult> {
    if let Err(err) = fs::remove_dir(path) {
        if err.kind() == std::io::ErrorKind::NotFound {
            return Ok(RemoveDirIfEmptyResult::Removed);
        }
        if err.kind() == std::io::ErrorKind::DirectoryNotEmpty {
            return Ok(RemoveDirIfEmptyResult::NotEmpty);
        }
        return Err(err)
            .wrap_err_with(|| format!("failed to remove directory: {}", path.display()));
    }
    Ok(RemoveDirIfEmptyResult::Removed)
}

pub(crate) fn create_dir_all(path: &Path) -> eyre::Result<()> {
    fs::create_dir_all(path)
        .wrap_err_with(|| format!("failed to create directory: {}", path.display()))
}

pub(crate) fn create_dir(path: &Path) -> eyre::Result<()> {
    fs::create_dir(path).wrap_err_with(|| format!("failed to create directory: {}", path.display()))
}

pub(crate) fn remove_file(path: &Path) -> eyre::Result<()> {
    fs::remove_file(path).wrap_err_with(|| format!("failed to remove file: {}", path.display()))
}
