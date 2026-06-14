use color_eyre::eyre;

use crate::{report::ExecResult, scenario::ScenarioParameters};

const PKG_SPEC: &str = "hackgen";

pub(super) fn run(
    params: &ScenarioParameters,
    exec_results: &mut Vec<ExecResult>,
) -> eyre::Result<()> {
    super::exec_foton(params, exec_results, |cmd| {
        cmd.args(["list", "--exit-on-lock"]);
    })?
    .ensure_success()?
    .ensure_stdout(str::is_empty)?;

    super::exec_foton(params, exec_results, |cmd| {
        cmd.args(["install", "--no-confirm", "--exit-on-lock", PKG_SPEC]);
    })?
    .ensure_success()?;

    super::exec_foton(params, exec_results, |cmd| {
        cmd.args(["list", "--exit-on-lock"]);
    })?
    .ensure_success()?
    .ensure_stdout(|stdout| stdout.lines().any(|line| line.starts_with("hackgen@")))?;

    super::exec_foton(params, exec_results, |cmd| {
        cmd.args(["uninstall", "--no-confirm", "--exit-on-lock", PKG_SPEC]);
    })?
    .ensure_success()?;

    super::exec_foton(params, exec_results, |cmd| {
        cmd.args(["list", "--exit-on-lock"]);
    })?
    .ensure_success()?
    .ensure_stdout(str::is_empty)?;

    Ok(())
}
