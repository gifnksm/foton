use cargo_metadata::camino::Utf8PathBuf;
use color_eyre::eyre::{self, ensure};
use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator as _;

use crate::{
    bootstrap::SandboxBootstrapConfig,
    report::{ReportContext, RunKind, RunReport},
    scenario::{context::ScenarioContext, params::ScenarioParameters},
    util::env::{self as env_util, FOTON_EXE_PATH_ENVVAR_NAME, FOTON_FIXTURE_DIR_PATH_ENVVAR_NAME},
};

mod cases;
mod context;
mod model;
mod params;

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
    InstallUninstall,
    Update,
    ManifestCheck,
    InstallFailure,
}

/// Commands for running scenarios.
#[derive(clap::Subcommand)]
pub(crate) enum ScenarioCommand {
    /// Run a scenario and print a summary of the captured outputs.
    Run {
        /// Dangerous: bypass the isolation safeguard and run the scenario directly.
        ///
        /// These scenarios are development and testing helpers, not commands meant
        /// for routine execution on the current environment. They may install,
        /// uninstall, or update fonts, rewrite Windows registry entries under HKCU,
        /// and otherwise modify state on the current system.
        ///
        /// Running a scenario without isolation may seriously damage the current
        /// user environment and may leave the current system broken or unusable
        /// for the current user.
        ///
        /// Use this only if you fully understand exactly what will be executed and
        /// exactly what state may be modified.
        ///
        /// Prefer `cargo xtask sandbox run --scenario <scenario>` unless you
        /// intentionally need to run without isolation.
        #[clap(
            long = "allow-unsafe-scenario-run-directly-without-isolation",
            env = "FOTON_ALLOW_UNSAFE_SCENARIO_RUN_DIRECTLY_WITHOUT_ISOLATION"
        )]
        allow_run_without_isolation: bool,
        /// Scenario to run.
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
    #[clap(long, env = FOTON_EXE_PATH_ENVVAR_NAME)]
    foton_exe: Option<Utf8PathBuf>,
    /// Directory containing test fixtures. If omitted, the default fixture directory is used.
    #[clap(long, env = FOTON_FIXTURE_DIR_PATH_ENVVAR_NAME)]
    fixture_dir: Option<Utf8PathBuf>,
    /// Directory where scenario outputs are written. If omitted, a temporary directory is used.
    #[clap(long)]
    output_dir: Option<Utf8PathBuf>,
}

pub(crate) fn dispatch(command: &ScenarioCommand) -> eyre::Result<()> {
    match command {
        ScenarioCommand::Run {
            allow_run_without_isolation,
            scenario,
            args,
        } => {
            let allow_run = *allow_run_without_isolation
                || env_util::is_in_github_actions_runner()
                || env_util::is_in_windows_sandbox_environment();
            ensure!(
                allow_run,
                "running scenarios without isolation is not allowed. Use `cargo xtask sandbox run --scenario {scenario}`."
            );

            let (_tempdir_guard, params) = ScenarioParameters::from_args(args)?;
            let (res, report) =
                RunReport::capture(params.run_id, RunKind::Scenario(*scenario), |cx| {
                    let cx = ScenarioContext::new(cx, &params);
                    cases::run(&cx, *scenario)
                });
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

pub(crate) fn run_in_sandbox(
    cx: &ReportContext,
    config: &SandboxBootstrapConfig,
    scenario: Scenario,
) -> eyre::Result<()> {
    let params = ScenarioParameters::from(config);
    let cx = ScenarioContext::new(cx, &params);
    cases::run(&cx, scenario)
}
