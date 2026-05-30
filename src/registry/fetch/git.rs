use git2::{
    FetchOptions, Repository,
    build::{CheckoutBuilder, RepoBuilder},
};
use snafu::ResultExt as _;
use url::Url;

use super::FetchRegistryError;
use crate::{
    registry::{
        RegistryId, RegistryIndex,
        fetch::{
            GitEmptyHeadPropertySnafu, GitOperationSnafu, GitRemoteUrlMismatchSnafu,
            GitUncommittedChangesSnafu, OpenIndexSnafu,
        },
    },
    util::{app_dirs::AppDirs, path::AbsolutePath},
};

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
    let op_snafu_owned = |operation: String| GitOperationSnafu {
        id,
        path,
        operation,
    };
    let custom = git2::Error::from_str;
    let empty_head_prop = |property| GitEmptyHeadPropertySnafu { id, path, property };

    let repo = Repository::open(path).with_context(|_| op_snafu("open repository"))?;
    let statuses = repo
        .statuses(None)
        .with_context(|_| op_snafu("get repository status"))?;
    snafu::ensure!(statuses.is_empty(), GitUncommittedChangesSnafu { id, path });

    let head = repo.head().with_context(|_| op_snafu("get HEAD"))?;
    let branch_full_name = head
        .name()
        .with_context(|_| op_snafu("get HEAD reference name"))?;
    snafu::ensure!(
        !branch_full_name.is_empty(),
        empty_head_prop("reference name")
    );
    let branch_short_name = head
        .shorthand()
        .with_context(|_| op_snafu("get HEAD shorthand"))?;
    snafu::ensure!(!branch_short_name.is_empty(), empty_head_prop("shorthand"));

    let mut remote = repo
        .find_remote(REMOTE_NAME)
        .with_context(|_| op_snafu_owned(format!("find remote '{REMOTE_NAME}'")))?;
    let remote_url = remote.url().with_context(|_| op_snafu("get remote URL"))?;

    snafu::ensure!(
        remote_url == url.as_str(),
        GitRemoteUrlMismatchSnafu { id, path }
    );

    let mut fetch_opts = FetchOptions::new();
    fetch_opts.depth(1);
    remote
        .fetch(<&[&str]>::default(), Some(&mut fetch_opts), None)
        .with_context(|_| op_snafu_owned(format!("fetch from remote '{REMOTE_NAME}'")))?;

    let remote_ref = format!("refs/remotes/{REMOTE_NAME}/{branch_short_name}");
    let remote_oid = repo
        .find_reference(&remote_ref)
        .with_context(|_| op_snafu("find remote branch reference"))?
        .target()
        .ok_or_else(|| custom("remote branch reference does not point to a commit"))
        .with_context(|_| op_snafu("find remote branch reference"))?;

    let mut reference = repo
        .find_reference(branch_full_name)
        .with_context(|_| op_snafu("find HEAD reference"))?;
    reference
        .set_target(remote_oid, "fast-forward")
        .with_context(|_| op_snafu("update HEAD reference"))?;

    let mut checkout_builder = CheckoutBuilder::new();
    checkout_builder.force();
    repo.checkout_head(Some(&mut checkout_builder))
        .with_context(|_| op_snafu("checkout HEAD"))?;

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
        .with_context(|_| op_snafu("clone git repository"))?;
    Ok(())
}
