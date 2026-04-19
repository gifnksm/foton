use cargo_metadata::camino::Utf8PathBuf;
use color_eyre::eyre;
use serde::{Deserialize, Serialize};

use crate::{
    report::{ExecResult, RunId, RunKind, RunReport},
    util::fs as fs_util,
};

mod help_check;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum, derive_more::Display, Serialize, Deserialize,
)]
#[display(rename_all = "kebab-case")]
#[serde(rename_all = "snake_case")]
pub(crate) enum Scenario {
    HelpCheck,
}

/// Commands for running scenarios.
#[derive(clap::Subcommand)]
pub(crate) enum ScenarioCommand {
    /// Run a scenario and write its outputs to the specified directory.
    Run {
        /// Scenario to run.
        #[clap(long)]
        scenario: Scenario,
        #[clap(flatten)]
        args: RunArgs,
    },
}

/// Common arguments for running a scenario.
#[derive(clap::Args)]
pub(crate) struct RunArgs {
    /// Path to the `foton` executable to run inside the scenario.
    #[clap(long)]
    foton_exe: Utf8PathBuf,
    /// Directory where scenario outputs are written.
    #[clap(long)]
    output_dir: Utf8PathBuf,
}

pub(crate) fn dispatch(command: &ScenarioCommand) -> eyre::Result<()> {
    match command {
        ScenarioCommand::Run { scenario, args } => {
            let params = args.parameters();
            let (res, report) = RunReport::capture(
                params.run_id,
                RunKind::Scenario(*scenario),
                |exec_results| run(*scenario, &params, exec_results),
            );
            report.print_summary();
            res
        }
    }
}

impl RunArgs {
    fn parameters(&self) -> ScenarioParameters {
        ScenarioParameters {
            foton_exe: self.foton_exe.clone(),
            output_dir: self.output_dir.clone(),
            run_id: RunId::new(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ScenarioParameters {
    pub(crate) foton_exe: Utf8PathBuf,
    pub(crate) output_dir: Utf8PathBuf,
    pub(crate) run_id: RunId,
}

pub(crate) fn run(
    scenario: Scenario,
    params: &ScenarioParameters,
    exec_results: &mut Vec<ExecResult>,
) -> eyre::Result<()> {
    fs_util::create_dir_all("output directory", &params.output_dir)?;

    match scenario {
        Scenario::HelpCheck => help_check::run(params, exec_results),
    }
}
