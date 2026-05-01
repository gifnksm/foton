use git2::{
    FetchOptions, Repository,
    build::{CheckoutBuilder, RepoBuilder},
};
use snafu::ResultExt as _;
use url::Url;

use crate::{
    registry::{
        RegistryId, RegistryIndex,
        fetch::{
            GitOperationSnafu, GitRemoteUrlMismatchSnafu, GitUncommittedChangesSnafu,
            OpenIndexSnafu,
        },
    },
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
    let index = RegistryIndex::open(id.clone(), registry_dir.as_path().to_path_buf()).context(
        OpenIndexSnafu {
            id,
            path: registry_dir,
        },
    )?;
    Ok(index)
}

pub(in crate::registry) fn fetch_existing(
    id: &RegistryId,
    url: &Url,
    path: &AbsolutePath,
) -> Result<(), FetchRegistryError> {
    const REMOTE_NAME: &str = "origin";

    let op_snafu = |operation| GitOperationSnafu {
        id,
        path,
        operation,
    };
    let custom = git2::Error::from_str;

    let repo = Repository::open(path).context(op_snafu("open repository"))?;
    let statuses = repo
        .statuses(None)
        .context(op_snafu("get repository status"))?;
    snafu::ensure!(statuses.is_empty(), GitUncommittedChangesSnafu { id, path });

    let head = repo.head().context(op_snafu("get repository head"))?;
    let branch_full_name = head
        .name()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| custom("repository head reference name is empty or not valid UTF-8 string"))
        .context(op_snafu("get repository head name"))?;
    let branch_short_name = head
        .shorthand()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| custom("repository head shorthand is empty or not a valid UTF-8 string"))
        .context(op_snafu("get repository head name"))?;

    let mut remote = repo
        .find_remote(REMOTE_NAME)
        .context(op_snafu("find git remote"))?;

    snafu::ensure!(
        remote.url() == Some(url.as_str()),
        GitRemoteUrlMismatchSnafu { id, path }
    );

    let mut fetch_opts = FetchOptions::new();
    fetch_opts.depth(1);
    remote
        .fetch(<&[&str]>::default(), Some(&mut fetch_opts), None)
        .context(op_snafu("fetch from git remote"))?;

    let remote_ref = format!("refs/remotes/{REMOTE_NAME}/{branch_short_name}");
    let remote_oid = repo
        .find_reference(&remote_ref)
        .context(op_snafu("find remote branch reference"))?
        .target()
        .ok_or_else(|| custom("remote branch reference does not point to a commit"))
        .context(op_snafu("find remote branch reference"))?;

    let mut reference = repo
        .find_reference(branch_full_name)
        .context(op_snafu("find head reference"))?;
    reference
        .set_target(remote_oid, "fast-forward")
        .context(op_snafu("update head reference"))?;

    let mut checkout_builder = CheckoutBuilder::new();
    checkout_builder.force();
    repo.checkout_head(Some(&mut checkout_builder))
        .context(op_snafu("checkout head"))?;

    Ok(())
}

pub(in crate::registry) fn fetch_new(
    id: &RegistryId,
    url: &Url,
    path: &AbsolutePath,
) -> Result<(), FetchRegistryError> {
    let op_snafu = |operation| GitOperationSnafu {
        id,
        path,
        operation,
    };

    let mut fetch_opts = FetchOptions::new();
    fetch_opts.depth(1);

    let _repo = RepoBuilder::new()
        .fetch_options(fetch_opts)
        .clone(url.as_str(), path.as_path())
        .context(op_snafu("clone git repository"))?;
    Ok(())
}
