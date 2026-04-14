use std::process::Command;

use color_eyre::eyre::{self, ensure};

use crate::scenario::RunArgs;

pub(super) fn run(args: &RunArgs) -> eyre::Result<()> {
    let res = super::exec_command(
        "foton",
        &args.output_dir,
        Command::new(&args.foton_exe).arg("--help"),
    )?;

    ensure!(
        res.status.success(),
        "foton exitcode is not 0: {}",
        res.status
    );
    ensure!(
        res.stdout()?.contains("Usage:"),
        "foton stdout does not contain `Usage:`"
    );
    ensure!(res.stderr()?.is_empty(), "foton stderr is not empty");

    Ok(())
}
