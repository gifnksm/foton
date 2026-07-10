use std::{env, sync::LazyLock};

use cargo_metadata::camino::{Utf8Path, Utf8PathBuf};
use color_eyre::eyre::{self, OptionExt as _, WrapErr as _, eyre};
use directories::ProjectDirs;

const APP_QUALIFIER: &str = "";
const APP_ORGANIZATION: &str = "io.github.gifnksm";
const APP_APPLICATION: &str = "foton";

static FOTON_PROJECT_DIRS: LazyLock<Option<ProjectDirs>> =
    LazyLock::new(|| ProjectDirs::from(APP_QUALIFIER, APP_ORGANIZATION, APP_APPLICATION));

pub(crate) fn foton_data_dir() -> eyre::Result<&'static Utf8Path> {
    let data_local_dir = FOTON_PROJECT_DIRS
        .as_ref()
        .ok_or_eyre("failed to get FOTON project directories")?
        .data_local_dir();
    data_local_dir.try_into().wrap_err_with(|| {
        format!(
            "invalid UTF-8 path for FOTON data directory: {}",
            data_local_dir.display()
        )
    })
}

pub(crate) fn foton_db_path() -> eyre::Result<Utf8PathBuf> {
    Ok(foton_data_dir()?.join("db.json"))
}

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

pub(crate) fn fixture_dir() -> eyre::Result<Utf8PathBuf> {
    Ok(repository_root_dir()?.join("xtask").join("fixture"))
}

pub(crate) fn is_in_github_actions_runner() -> bool {
    env::var_os("GITHUB_ACTIONS").is_some_and(|v| v == "true")
}

pub(crate) const FOTON_EXECUTION_ENVIRONMENT_ENVVAR_NAME: &str = "FOTON_EXECUTION_ENVIRONMENT";
pub(crate) const FOTON_EXECUTION_ENVIRONMENT_WINDOWS_SANDBOX: &str = "windows-sandbox";

pub(crate) fn is_in_windows_sandbox_environment() -> bool {
    env::var_os(FOTON_EXECUTION_ENVIRONMENT_ENVVAR_NAME)
        .is_some_and(|v| v == FOTON_EXECUTION_ENVIRONMENT_WINDOWS_SANDBOX)
}

pub(crate) const FOTON_EXE_PATH_ENVVAR_NAME: &str = "FOTON_EXE_PATH";
pub(crate) const FOTON_FIXTURE_DIR_PATH_ENVVAR_NAME: &str = "FOTON_FIXTURE_DIR";
