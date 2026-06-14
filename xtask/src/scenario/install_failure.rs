use color_eyre::eyre;

use crate::{
    report::{self, ExecResult},
    scenario::ScenarioParameters,
};

pub(super) fn run(
    params: &ScenarioParameters,
    exec_results: &mut Vec<ExecResult>,
) -> eyre::Result<()> {
    let fixture_dir = params.fixture_dir.join("install_failure");

    super::exec_foton(params, exec_results, |cmd| {
        cmd.args([
            "install",
            "--no-confirm",
            "--warnings-as-errors",
            "--exit-on-lock",
            "--manifest",
        ])
        .arg(fixture_dir.join("manifest.toml"));
    })?
    .ensure_exit_code(1)?
    .ensure_stdout(str::is_empty)?
    .ensure_stderr(report::contains_error_line)?
    .ensure_stderr(|stderr| {
        stderr
            .lines()
            .any(|line| line.contains("failed to install"))
    })?;

    Ok(())
}
