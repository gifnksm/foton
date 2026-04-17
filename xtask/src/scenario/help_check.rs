use std::process::Command;

use color_eyre::eyre::{self, ensure};

use crate::scenario::ScenarioParameters;

pub(super) fn run(params: &ScenarioParameters) -> eyre::Result<()> {
    let res = super::exec_command(
        "foton",
        &params.output_dir,
        Command::new(&params.foton_exe).arg("--help"),
    )?;

    ensure!(
        res.status.success(),
        "foton exited with non-zero status: {}",
        res.status
    );
    ensure!(
        res.stdout()?.contains("Usage:"),
        "foton stdout does not contain `Usage:`"
    );
    ensure!(res.stderr()?.is_empty(), "foton stderr is not empty");

    Ok(())
}
