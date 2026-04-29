use std::process::Command;

use cargo_metadata::camino::Utf8PathBuf;
use color_eyre::eyre::{self, WrapErr as _, eyre};
use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator as _;
use tempfile::TempDir;

use crate::{
    report::{ExecResult, RunId, RunKind, RunReport},
    util::{build, env as env_util, fs as fs_util},
};

mod help_check;
mod smoke_test;

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    clap::ValueEnum,
    derive_more::Display,
    Serialize,
    Deserialize,
    strum::EnumIter,
    strum::IntoStaticStr,
)]
#[display(rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Scenario {
    HelpCheck,
    SmokeTest,
}

/// Commands for running scenarios.
#[derive(clap::Subcommand)]
pub(crate) enum ScenarioCommand {
    /// Run a scenario and print a summary of the captured outputs.
    Run {
        /// Scenario to run.
        #[clap(long)]
        scenario: Scenario,
        #[clap(flatten)]
        args: RunArgs,
    },
    /// List the available scenarios.
    List {
        /// Print the scenario list as JSON.
        #[clap(long)]
        json: bool,
    },
}

/// Common arguments for running a scenario.
#[derive(clap::Args)]
pub(crate) struct RunArgs {
    /// Path to the `foton` executable to run inside the scenario. If omitted, `foton` is built automatically.
    #[clap(long)]
    foton_exe: Option<Utf8PathBuf>,
    /// Directory where scenario outputs are written. If omitted, a temporary directory is used.
    #[clap(long)]
    output_dir: Option<Utf8PathBuf>,
    /// Package registry directory used by the scenario. If omitted, the repository `packages` directory is used.
    #[clap(long)]
    registry: Option<Utf8PathBuf>,
}

pub(crate) fn dispatch(command: &ScenarioCommand) -> eyre::Result<()> {
    match command {
        ScenarioCommand::Run { scenario, args } => {
            let (_tempdir_guard, params) = args.build_parameters()?;
            let (res, report) = RunReport::capture(
                params.run_id,
                RunKind::Scenario(*scenario),
                |exec_results| run(*scenario, &params, exec_results),
            );
            report.print_summary();
            res
        }
        ScenarioCommand::List { json } => {
            if *json {
                let scenarios = Scenario::iter().map(|s| s.to_string()).collect::<Vec<_>>();
                println!("{}", serde_json::to_string_pretty(&scenarios)?);
            } else {
                for scenario in Scenario::iter() {
                    println!("{scenario}");
                }
            }
            Ok(())
        }
    }
}

impl RunArgs {
    fn build_parameters(&self) -> eyre::Result<(Option<TempDir>, ScenarioParameters)> {
        let foton_exe = if let Some(path) = self.foton_exe.clone() {
            path
        } else {
            build::build_foton_exe()?
        };
        let registry_dir = if let Some(path) = self.registry.clone() {
            path
        } else {
            env_util::registry_dir()?
        };
        let (tempdir_guard, output_dir) = if let Some(output_dir) = &self.output_dir {
            fs_util::create_dir_all("output directory", output_dir)?;
            (None, output_dir.clone())
        } else {
            let tempdir = TempDir::new().wrap_err("failed to create temporary output directory")?;
            let path = Utf8PathBuf::from_path_buf(tempdir.path().to_owned()).map_err(|path| {
                eyre!(
                    "failed to convert temporary output directory path to UTF-8: {}",
                    path.display()
                )
            })?;
            (Some(tempdir), path)
        };
        Ok((
            tempdir_guard,
            ScenarioParameters {
                foton_exe,
                registry_dir,
                output_dir,
                run_id: RunId::new(),
            },
        ))
    }
}

#[derive(Debug)]
pub(crate) struct ScenarioParameters {
    pub(crate) foton_exe: Utf8PathBuf,
    pub(crate) registry_dir: Utf8PathBuf,
    pub(crate) output_dir: Utf8PathBuf,
    pub(crate) run_id: RunId,
}

pub(crate) fn run(
    scenario: Scenario,
    params: &ScenarioParameters,
    exec_results: &mut Vec<ExecResult>,
) -> eyre::Result<()> {
    match scenario {
        Scenario::HelpCheck => help_check::run(params, exec_results),
        Scenario::SmokeTest => smoke_test::run(params, exec_results),
    }
}

fn exec_foton<'a, F>(
    params: &ScenarioParameters,
    exec_results: &'a mut Vec<ExecResult>,
    f: F,
) -> eyre::Result<&'a mut ExecResult>
where
    F: FnOnce(&mut Command),
{
    let mut cmd = Command::new(&params.foton_exe);
    f(&mut cmd);
    let res = crate::util::process::exec_command("foton", &params.output_dir, &mut cmd)?;
    Ok(exec_results.push_mut(res))
}
