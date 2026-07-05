use color_eyre::eyre;

use crate::{report, scenario::ScenarioContext};

pub(super) fn run(cx: &ScenarioContext<'_>) -> eyre::Result<()> {
    let fixture_dir = cx.params.fixture_dir.join("manifest_check");

    cx.exec_foton(|cmd| {
        cmd.args(["manifest", "check", "--no-confirm", "--warnings-as-errors"])
            .arg(fixture_dir.join("manifest_without_warning.toml"));
    })?
    .ensure_success()?
    .ensure_stdout(str::is_empty)?
    .ensure_stderr(report::not_contains_error_line)?
    .ensure_stderr(report::not_contains_warning_line)?;

    cx.exec_foton(|cmd| {
        cmd.args(["manifest", "check", "--no-confirm"])
            .arg(fixture_dir.join("manifest_with_warning.toml"));
    })?
    .ensure_success()?
    .ensure_stdout(str::is_empty)?
    .ensure_stderr(report::contains_warning_line)?
    .ensure_stderr(report::not_contains_error_line)?;

    cx.exec_foton(|cmd| {
        cmd.args(["manifest", "check", "--no-confirm", "--warnings-as-errors"])
            .arg(fixture_dir.join("manifest_with_warning.toml"));
    })?
    .ensure_exit_code(1)?
    .ensure_stdout(str::is_empty)?
    .ensure_stderr(report::not_contains_warning_line)?
    .ensure_stderr(report::contains_error_line)?;

    Ok(())
}
