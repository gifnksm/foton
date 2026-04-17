use std::{
    process::{Command, ExitStatus},
    sync::atomic::{AtomicUsize, Ordering},
};

use cargo_metadata::camino::{Utf8Path, Utf8PathBuf};
use color_eyre::eyre::{self, WrapErr as _};
use serde::{Deserialize, Serialize};

use crate::{
    fs_util,
    report::{ScenarioOutcome, ScenarioReport},
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
    /// Optional path to the JSON report file.
    ///
    /// If omitted, `<output-dir>/report.json` is used.
    #[clap(long)]
    report: Option<Utf8PathBuf>,
}

pub(crate) fn dispatch(command: &ScenarioCommand) -> eyre::Result<()> {
    match command {
        ScenarioCommand::Run { scenario, args } => run(*scenario, &args.parameters()),
    }
}

impl RunArgs {
    fn parameters(&self) -> ScenarioParameters {
        ScenarioParameters {
            foton_exe: self.foton_exe.clone(),
            output_dir: self.output_dir.clone(),
            report: self
                .report
                .clone()
                .unwrap_or_else(|| self.output_dir.join("report.json")),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ScenarioParameters {
    pub(crate) foton_exe: Utf8PathBuf,
    pub(crate) output_dir: Utf8PathBuf,
    pub(crate) report: Utf8PathBuf,
}

pub(crate) fn run(scenario: Scenario, params: &ScenarioParameters) -> eyre::Result<()> {
    fs_util::create_dir_all("output directory", &params.output_dir)?;

    let res = match scenario {
        Scenario::HelpCheck => help_check::run(params),
    };

    write_scenario_report(scenario, &params.report, &res)?;

    res
}

fn exec_command<S>(name: S, output_dir: &Utf8Path, cmd: &mut Command) -> eyre::Result<ExecResult>
where
    S: Into<String>,
{
    static EXEC_COUNTER: AtomicUsize = AtomicUsize::new(0);

    let name = name.into();

    let id = EXEC_COUNTER.fetch_add(1, Ordering::Relaxed);
    let file_prefix = format_args!("{id}.{name}");

    let stdout_path = output_dir.join(format!("{file_prefix}.stdout.txt"));
    let stderr_path = output_dir.join(format!("{file_prefix}.stderr.txt"));
    let status_path = output_dir.join(format!("{file_prefix}.status.txt"));

    let stdout_file = fs_util::create_file(format_args!("{name} stdout"), &stdout_path)?;
    let stderr_file = fs_util::create_file(format_args!("{name} stderr"), &stderr_path)?;

    let status = cmd
        .stdout(stdout_file)
        .stderr(stderr_file)
        .status()
        .wrap_err_with(|| format!("failed to execute {name} (exec #{id})"))?;

    fs_util::write(
        format_args!("{name} status"),
        &status_path,
        status.to_string(),
    )?;

    Ok(ExecResult {
        name,
        status,
        stdout_path,
        stderr_path,
    })
}

#[derive(Debug)]
struct ExecResult {
    name: String,
    status: ExitStatus,
    stdout_path: Utf8PathBuf,
    stderr_path: Utf8PathBuf,
}

impl ExecResult {
    fn stdout(&self) -> eyre::Result<String> {
        fs_util::read_to_string(format_args!("{} stdout", self.name), &self.stdout_path)
    }

    fn stderr(&self) -> eyre::Result<String> {
        fs_util::read_to_string(format_args!("{} stderr", self.name), &self.stderr_path)
    }
}

fn write_scenario_report(
    scenario: Scenario,
    path: &Utf8Path,
    res: &eyre::Result<()>,
) -> eyre::Result<()> {
    let outcome = match res {
        Ok(()) => ScenarioOutcome::Success,
        Err(err) => ScenarioOutcome::Failure {
            error: err.to_string(),
            sources: err.chain().skip(1).map(ToString::to_string).collect(),
        },
    };
    let report = ScenarioReport { outcome };
    fs_util::write_json(
        format_args!("scenario {scenario} result (JSON)"),
        path,
        &report,
    )?;
    Ok(())
}
