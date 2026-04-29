use std::process::Command;

use color_eyre::eyre::{self, ensure};

use crate::{report::ExecResult, scenario::ScenarioParameters, util::process as process_util};

const PKG_SPEC: &str = "yuru7/hackgen";

pub(super) fn run(
    params: &ScenarioParameters,
    exec_results: &mut Vec<ExecResult>,
) -> eyre::Result<()> {
    let res = process_util::exec_command(
        "foton",
        &params.output_dir,
        Command::new(&params.foton_exe).args([
            "install",
            PKG_SPEC,
            "--registry",
            params.registry_dir.as_str(),
        ]),
    )?;
    let res = exec_results.push_mut(res);
    ensure!(
        res.success,
        "foton exited with non-zero status: {}",
        res.exit_status
    );

    let res = process_util::exec_command(
        "foton",
        &params.output_dir,
        Command::new(&params.foton_exe).args(["uninstall", PKG_SPEC]),
    )?;
    let res = exec_results.push_mut(res);
    ensure!(
        res.success,
        "foton exited with non-zero status: {}",
        res.exit_status
    );

    Ok(())
}
