use std::path::PathBuf;

use snafu::Snafu;

use crate::{
    registry::{RegistryId, RegistryIndex, RegistryIndexError, RegistrySource, RegistrySpec},
    util::{app_dirs::AppDirs, macros::concat_line},
};

mod git;
mod local;

#[derive(Debug, Snafu)]
pub(crate) enum FetchRegistryError {
    #[snafu(display("failed to open registry index of registry `{id}`: {path}", path = path.display()))]
    OpenIndex {
        id: RegistryId,
        path: PathBuf,
        #[snafu(source(from(RegistryIndexError, Box::new)))]
        source: Box<RegistryIndexError>,
    },
    #[snafu(display("failed to perform git operation `{operation}` for registry `{id}`: {path}", path = path.display()))]
    GitOperation {
        id: RegistryId,
        path: PathBuf,
        operation: String,
        #[snafu(source(from(git2::Error, Box::new)))]
        source: Box<git2::Error>,
    },
    #[snafu(display(
        concat_line!(
            "registry `{id}` has an empty {property} in HEAD: {path}",
            "remove the cached registry directory and retry",
        ),
        id = id,
        property = property,
        path = path.display(),
    ))]
    GitEmptyHeadProperty {
        id: RegistryId,
        path: PathBuf,
        property: &'static str,
    },
    #[snafu(display("registry `{id}` has uncommitted changes: {path}", path = path.display()))]
    GitUncommittedChanges { id: RegistryId, path: PathBuf },
    #[snafu(display(
        concat_line!(
            "registry `{id}` has a different remote URL configured than the cached repository: {path}",
            "remove the cached registry directory and retry",
        ),
        id = id,
        path = path.display(),
    ))]
    GitRemoteUrlMismatch { id: RegistryId, path: PathBuf },
}

pub(crate) fn fetch_registry(
    app_dirs: &AppDirs,
    registry: &RegistrySpec,
) -> Result<RegistryIndex, FetchRegistryError> {
    match &registry.source {
        RegistrySource::Git(url) => git::fetch_registry(app_dirs, registry.id(), url),
        RegistrySource::Local(path) => local::fetch_registry(registry.id(), path),
    }
}
