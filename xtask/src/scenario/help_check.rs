use std::process::Command;

use color_eyre::eyre::{self, ensure};

use crate::{report::ExecResult, scenario::ScenarioParameters, util::process as process_util};

pub(super) fn run(
    params: &ScenarioParameters,
    exec_results: &mut Vec<ExecResult>,
) -> eyre::Result<()> {
    let res = process_util::exec_command(
        "foton",
        &params.output_dir,
        Command::new(&params.foton_exe).arg("--help"),
    )?;

    let res = exec_results.push_mut(res);

    ensure!(
        res.success,
        "foton exited with non-zero status: {}",
        res.exit_status
    );
    ensure!(
        res.stdout.contains("Usage:"),
        "foton stdout does not contain `Usage:`"
    );
    ensure!(res.stderr.is_empty(), "foton stderr is not empty");

    Ok(())
}
