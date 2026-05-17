use color_eyre::eyre;

use crate::{
    report::{self, ExecResult},
    scenario::ScenarioParameters,
};

pub(super) fn run(
    params: &ScenarioParameters,
    exec_results: &mut Vec<ExecResult>,
) -> eyre::Result<()> {
    let fixture_dir = params.fixture_dir.join("manifest_check");

    super::exec_foton(params, exec_results, |cmd| {
        cmd.args(["manifest", "check", "--no-confirm", "--warnings-as-errors"])
            .arg(fixture_dir.join("manifest_without_warning.toml"));
    })?
    .ensure_success()?
    .ensure_stdout(str::is_empty)?
    .ensure_stderr(report::not_contains_error_line)?
    .ensure_stderr(report::not_contains_warning_line)?;

    super::exec_foton(params, exec_results, |cmd| {
        cmd.args(["manifest", "check", "--no-confirm"])
            .arg(fixture_dir.join("manifest_with_warning.toml"));
    })?
    .ensure_success()?
    .ensure_stdout(str::is_empty)?
    .ensure_stderr(report::contains_warning_line)?
    .ensure_stderr(report::not_contains_error_line)?;

    super::exec_foton(params, exec_results, |cmd| {
        cmd.args(["manifest", "check", "--no-confirm", "--warnings-as-errors"])
            .arg(fixture_dir.join("manifest_with_warning.toml"));
    })?
    .ensure_exit_code(1)?
    .ensure_stdout(str::is_empty)?
    .ensure_stderr(report::not_contains_warning_line)?
    .ensure_stderr(report::contains_error_line)?;

    Ok(())
}
