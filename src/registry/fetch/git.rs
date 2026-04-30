use git2::{
    FetchOptions, Repository,
    build::{CheckoutBuilder, RepoBuilder},
};
use url::Url;

use crate::{
    registry::{RegistryId, RegistryIndex},
    util::{app_dirs::AppDirs, path::AbsolutePath},
};

use super::FetchRegistryError;

pub(in crate::registry) fn fetch_registry(
    app_dirs: &AppDirs,
    id: &RegistryId,
    url: &Url,
) -> Result<RegistryIndex, FetchRegistryError> {
    let registry_dir = app_dirs.registry_cache_dir(id);
    if registry_dir.as_path().exists() {
        fetch_existing(id, url, &registry_dir)?;
    } else {
        fetch_new(id, url, &registry_dir)?;
    }
    let index = RegistryIndex::open(id.clone(), registry_dir.as_path().to_path_buf()).map_err(
        |source| {
            let id = id.clone();
            let path = registry_dir.clone();
            let source = Box::new(source);
            FetchRegistryError::OpenIndex { id, path, source }
        },
    )?;
    Ok(index)
}

fn error_git_operation(
    id: &RegistryId,
    path: &AbsolutePath,
    operation: &'static str,
    source: git2::Error,
) -> FetchRegistryError {
    let id = id.clone();
    let path = path.clone();
    let source = Box::new(source);
    FetchRegistryError::GitOperation {
        id,
        path,
        operation,
        source,
    }
}

pub(in crate::registry) fn fetch_existing(
    id: &RegistryId,
    url: &Url,
    path: &AbsolutePath,
) -> Result<(), FetchRegistryError> {
    const REMOTE_NAME: &str = "origin";

    let git_error = move |operation| move |source| error_git_operation(id, path, operation, source);
    let git_error_custom = move |operation, source| {
        error_git_operation(id, path, operation, git2::Error::from_str(source))
    };

    let repo = Repository::open(path).map_err(git_error("open repository"))?;
    let statuses = repo
        .statuses(None)
        .map_err(git_error("get repository status"))?;
    if !statuses.is_empty() {
        let id = id.clone();
        let path = path.clone();
        return Err(FetchRegistryError::GitUncommittedChanges { id, path });
    }

    let head = repo.head().map_err(git_error("get repository head"))?;
    let branch_full_name = head.name().filter(|s| !s.is_empty()).ok_or_else(|| {
        git_error_custom(
            "get repository head name",
            "repository head reference name is empty or not valid UTF-8 string",
        )
    })?;
    let branch_short_name = head.shorthand().filter(|s| !s.is_empty()).ok_or_else(|| {
        git_error_custom(
            "get repository head name",
            "repository head shorthand is empty or not a valid UTF-8 string",
        )
    })?;

    let mut remote = repo
        .find_remote(REMOTE_NAME)
        .map_err(git_error("find git remote"))?;

    if remote.url() != Some(url.as_str()) {
        let id = id.clone();
        let path = path.clone();
        return Err(FetchRegistryError::GitRemoteUrlMismatch { id, path });
    }

    let mut fetch_opts = FetchOptions::new();
    fetch_opts.depth(1);
    remote
        .fetch(<&[&str]>::default(), Some(&mut fetch_opts), None)
        .map_err(git_error("fetch from git remote"))?;

    let remote_ref = format!("refs/remotes/{REMOTE_NAME}/{branch_short_name}");
    let remote_oid = repo
        .find_reference(&remote_ref)
        .map_err(git_error("find remote branch reference"))?
        .target()
        .ok_or_else(|| {
            git_error_custom(
                "find remote branch reference",
                "remote branch reference does not point to a commit",
            )
        })?;

    let mut reference = repo
        .find_reference(branch_full_name)
        .map_err(git_error("find head reference"))?;
    reference
        .set_target(remote_oid, "fast-forward")
        .map_err(git_error("update head reference"))?;

    let mut checkout_builder = CheckoutBuilder::new();
    checkout_builder.force();
    repo.checkout_head(Some(&mut checkout_builder))
        .map_err(git_error("checkout head"))?;

    Ok(())
}

pub(in crate::registry) fn fetch_new(
    id: &RegistryId,
    url: &Url,
    path: &AbsolutePath,
) -> Result<(), FetchRegistryError> {
    let git_error = move |operation| move |source| error_git_operation(id, path, operation, source);

    let mut fetch_opts = FetchOptions::new();
    fetch_opts.depth(1);

    let _repo = RepoBuilder::new()
        .fetch_options(fetch_opts)
        .clone(url.as_str(), path.as_path())
        .map_err(git_error("clone git repository"))?;
    Ok(())
}
