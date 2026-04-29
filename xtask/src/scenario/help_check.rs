use color_eyre::eyre;

use crate::{report::ExecResult, scenario::ScenarioParameters};

pub(super) fn run(
    params: &ScenarioParameters,
    exec_results: &mut Vec<ExecResult>,
) -> eyre::Result<()> {
    super::exec_foton(params, exec_results, |cmd| {
        cmd.arg("--help");
    })?
    .ensure_success()?
    .ensure_stdout(|stdout| stdout.contains("Usage:"))?
    .ensure_stderr(str::is_empty)?;

    Ok(())
}
