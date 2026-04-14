use std::{
    fs,
    process::{Command, ExitStatus},
    sync::atomic::{AtomicUsize, Ordering},
};

use cargo_metadata::camino::{Utf8Path, Utf8PathBuf};
use color_eyre::eyre::{self, WrapErr as _};
use serde::{Deserialize, Serialize};

use crate::fs_util;

mod help_check;

#[derive(Debug, Clone, PartialEq, Eq, clap::ValueEnum, derive_more::Display)]
#[display(rename_all = "kebab-case")]
pub(crate) enum Scenario {
    HelpCheck,
}

#[derive(clap::Subcommand)]
pub(crate) enum ScenarioCommand {
    Run {
        #[clap(long)]
        scenario: Scenario,
        #[clap(flatten)]
        args: RunArgs,
    },
}

#[derive(clap::Args)]
pub(crate) struct RunArgs {
    #[clap(long)]
    foton_exe: Utf8PathBuf,
    #[clap(long)]
    output_dir: Utf8PathBuf,
}

pub(crate) fn dispatch(command: &ScenarioCommand) -> eyre::Result<()> {
    match command {
        ScenarioCommand::Run { scenario, args } => run(scenario, args),
    }
}

pub(crate) fn run(scenario: &Scenario, args: &RunArgs) -> eyre::Result<()> {
    fs::create_dir_all(&args.output_dir)
        .wrap_err_with(|| format!("failed to create output directory: {}", args.output_dir))?;

    let res = match scenario {
        Scenario::HelpCheck => help_check::run(args),
    };
    dump_report(scenario, &args.output_dir.join("report.json"), &res)?;
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

#[derive(Debug, Serialize, Deserialize)]
struct ScenarioReport {
    outcome: ScenarioOutcome,
}

#[derive(Debug, Serialize, Deserialize)]
enum ScenarioOutcome {
    Pass,
    Fail { error: String, sources: Vec<String> },
}

fn dump_report(scenario: &Scenario, path: &Utf8Path, res: &eyre::Result<()>) -> eyre::Result<()> {
    let outcome = match res {
        Ok(()) => ScenarioOutcome::Pass,
        Err(err) => ScenarioOutcome::Fail {
            error: err.to_string(),
            sources: err.chain().skip(1).map(|e| e.to_string()).collect(),
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
