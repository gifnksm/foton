use color_eyre::eyre;

use crate::{report::ExecResult, scenario::ScenarioParameters};

const PKG_SPEC_OLD: &str = "hackgen@2.9.1";

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
        cmd.args(["install", PKG_SPEC_OLD, "--no-confirm"]);
    })?
    .ensure_success()?;

    super::exec_foton(params, exec_results, |cmd| {
        cmd.args(["list"]);
    })?
    .ensure_success()?
    .ensure_stdout(|stdout| stdout.lines().any(|line| line.starts_with("hackgen@2.9.1")))?;

    super::exec_foton(params, exec_results, |cmd| {
        cmd.args(["update", "--no-confirm"]);
    })?
    .ensure_success()?;

    super::exec_foton(params, exec_results, |cmd| {
        cmd.args(["list"]);
    })?
    .ensure_success()?
    .ensure_stdout(|stdout| {
        stdout
            .lines()
            .all(|line| !line.starts_with("hackgen@2.9.1"))
    })?
    .ensure_stdout(|stdout| stdout.lines().any(|line| line.starts_with("hackgen@")))?;

    super::exec_foton(params, exec_results, |cmd| {
        cmd.args(["uninstall", "hackgen", "--no-confirm"]);
    })?
    .ensure_success()?;

    Ok(())
}
