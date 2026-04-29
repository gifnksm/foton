use color_eyre::eyre;

use crate::{report::ExecResult, scenario::ScenarioParameters};

const PKG_SPEC: &str = "yuru7/hackgen";

pub(super) fn run(
    params: &ScenarioParameters,
    exec_results: &mut Vec<ExecResult>,
) -> eyre::Result<()> {
    super::exec_foton(params, exec_results, |cmd| {
        cmd.args(["list"]);
    })?
    .ensure_success()?
    .ensure_stdout(str::is_empty)?;

    super::exec_foton(params, exec_results, |cmd| {
        cmd.args([
            "install",
            PKG_SPEC,
            "--registry",
            params.registry_dir.as_str(),
        ]);
    })?
    .ensure_success()?;

    super::exec_foton(params, exec_results, |cmd| {
        cmd.args(["list"]);
    })?
    .ensure_success()?
    .ensure_stdout(|stdout| {
        stdout
            .lines()
            .any(|line| line.starts_with("yuru7/hackgen@"))
    })?;

    super::exec_foton(params, exec_results, |cmd| {
        cmd.args(["uninstall", PKG_SPEC]);
    })?
    .ensure_success()?;

    super::exec_foton(params, exec_results, |cmd| {
        cmd.args(["list"]);
    })?
    .ensure_success()?
    .ensure_stdout(str::is_empty)?;

    Ok(())
}
