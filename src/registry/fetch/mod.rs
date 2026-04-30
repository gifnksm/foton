use crate::{
    registry::{RegistryId, RegistryIndex, RegistryIndexError, RegistrySource},
    util::{app_dirs::AppDirs, path::AbsolutePath},
};

mod git;
mod local;

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum FetchRegistryError {
    #[display("failed to open registry index of registry `{id}`: {path}", path = path.display())]
    OpenIndex {
        id: RegistryId,
        path: AbsolutePath,
        #[error(source)]
        source: Box<RegistryIndexError>,
    },
    #[display("failed to perform git operation `{operation}` for registry `{id}`: {path}", path = path.display())]
    GitOperation {
        id: RegistryId,
        path: AbsolutePath,
        operation: &'static str,
        #[error(source)]
        source: Box<git2::Error>,
    },
    #[display("registry `{id}` has uncommitted changes: {path}", path = path.display())]
    GitUncommittedChanges { id: RegistryId, path: AbsolutePath },
    #[display(
        "registry `{id}` has a different remote URL configured than the cached repository: {path}\nremove the cached registry directory and retry",
        path = path.display()
    )]
    GitRemoteUrlMismatch { id: RegistryId, path: AbsolutePath },
}

pub(crate) fn fetch_registry(
    app_dirs: &AppDirs,
    id: &RegistryId,
    source: &RegistrySource,
) -> Result<RegistryIndex, FetchRegistryError> {
    match source {
        RegistrySource::Git(url) => git::fetch_registry(app_dirs, id, url),
        RegistrySource::Local(path) => local::fetch_registry(id, path),
    }
}
