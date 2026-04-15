use std::env;

use cargo_metadata::camino::{Utf8Path, Utf8PathBuf};
use color_eyre::eyre::{self, OptionExt as _, WrapErr as _, eyre};

pub(crate) fn path_from_env(var: &str) -> eyre::Result<Option<Utf8PathBuf>> {
    let Some(path) = env::var_os(var) else {
        return Ok(None);
    };
    let path = Utf8PathBuf::from_os_string(path.clone())
        .ok()
        .ok_or_else(|| {
            eyre!("environment variable {var} contains a non-UTF-8 path value: {path:?}")
        })?;
    Ok(Some(path))
}

pub(crate) fn repository_root_dir() -> eyre::Result<&'static Utf8Path> {
    Utf8Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_eyre("failed to get repository root")
}

pub(crate) fn cargo_target_dir() -> eyre::Result<Utf8PathBuf> {
    if let Some(mut target_dir) = path_from_env("CARGO_TARGET_DIR")? {
        if target_dir.is_relative() {
            target_dir = repository_root_dir()?.join(target_dir);
        }
        return Ok(target_dir);
    }
    Ok(repository_root_dir()?.join("target"))
}

pub(crate) fn cargo_bin() -> eyre::Result<Utf8PathBuf> {
    Ok(path_from_env("CARGO")?.unwrap_or_else(|| "cargo".into()))
}

pub(crate) fn current_exe() -> eyre::Result<Utf8PathBuf> {
    let path = env::current_exe().wrap_err("failed to get current executable path")?;
    let path = Utf8PathBuf::from_path_buf(path)
        .ok()
        .ok_or_else(|| eyre!("failed to convert current executable path to UTF-8"))?;
    Ok(path)
}
