use cargo_metadata::camino::Utf8PathBuf;
use color_eyre::eyre::{self, WrapErr as _, ensure, eyre};
use strum::IntoEnumIterator as _;
use tempfile::TempDir;

use crate::{
    report::{RunId, RunKind, RunReport},
    scenario::{self, Scenario, ScenarioParameters},
    util::{
        build,
        env::{self as env_util, FOTON_EXE_PATH_ENVVAR_NAME, FOTON_FIXTURE_DIR_PATH_ENVVAR_NAME},
        fs as fs_util,
    },
};

/// Commands for running scenarios.
#[derive(clap::Subcommand)]
pub(in crate::command) enum ScenarioCommand {
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
pub(in crate::command) struct RunArgs {
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

pub(in crate::command) fn dispatch(command: &ScenarioCommand) -> eyre::Result<()> {
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

            let run_id = RunId::new();
            let params = build_scenario_parameters(args)?;
            let (_tempdir_guard, output_dir) = build_output_dir(args)?;

            let (res, report) =
                RunReport::capture(run_id, RunKind::Scenario(*scenario), &output_dir, |cx| {
                    scenario::run(cx, *scenario, &params)
                });
            eprintln!("Scenario Run Summary:");
            for line in report.to_string().lines() {
                eprintln!("  {line}");
            }
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

fn build_output_dir(args: &RunArgs) -> eyre::Result<(Option<TempDir>, Utf8PathBuf)> {
    if let Some(path) = &args.output_dir {
        fs_util::create_dir_all("output directory", path)?;
        Ok((None, path.to_owned()))
    } else {
        let tempdir = TempDir::new().wrap_err("failed to create temporary output directory")?;
        let path = Utf8PathBuf::from_path_buf(tempdir.path().to_owned()).map_err(|path| {
            eyre!(
                "failed to convert temporary output directory path to UTF-8: {}",
                path.display()
            )
        })?;
        Ok((Some(tempdir), path))
    }
}

fn build_scenario_parameters(args: &RunArgs) -> eyre::Result<ScenarioParameters> {
    let foton_exe = if let Some(path) = &args.foton_exe {
        path.to_owned()
    } else {
        build::build_foton_exe()?
    };

    let fixture_dir = if let Some(path) = &args.fixture_dir {
        path.to_owned()
    } else {
        env_util::fixture_dir()?
    };

    Ok(ScenarioParameters {
        foton_exe,
        fixture_dir,
    })
}
