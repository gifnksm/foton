use std::process::Command;

use cargo_metadata::camino::{Utf8Path, Utf8PathBuf};
use color_eyre::eyre::{self, WrapErr as _, bail};
use serde::{Deserialize, Serialize};

use crate::{
    report::{ExecResult, RunId, RunKind, RunReport},
    scenario::{self, Scenario},
    util::{fs as fs_util, process as process_util},
};

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct SandboxBootstrapConfig {
    pub(crate) foton_exe: Utf8PathBuf,
    pub(crate) xtask_exe: Utf8PathBuf,
    pub(crate) output_dir: Utf8PathBuf,
    pub(crate) complete_stamp: Utf8PathBuf,
    pub(crate) run_id: RunId,
    pub(crate) action: SandboxAction,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SandboxAction {
    Noop,
    RunTests {
        test_exes: Vec<Utf8PathBuf>,
        report_json: Utf8PathBuf,
    },
    RunScenario {
        scenario: Scenario,
        report_json: Utf8PathBuf,
    },
}

impl SandboxAction {
    pub(crate) fn to_kind(&self) -> RunKind {
        match self {
            Self::Noop => RunKind::Noop,
            Self::RunTests { .. } => RunKind::Test,
            Self::RunScenario { scenario, .. } => RunKind::Scenario(*scenario),
        }
    }
}

/// Arguments for sandbox bootstrap execution.
#[derive(Debug, clap::Args)]
pub(crate) struct BootstrapArgs {
    /// Path to the sandbox bootstrap config file.
    #[clap(long)]
    config: Utf8PathBuf,
    /// Run the bootstrap action in a child process with redirected stdout/stderr.
    #[clap(long)]
    run_as_child_process: bool,
}

pub(crate) fn dispatch(args: &BootstrapArgs) -> eyre::Result<()> {
    let config = fs_util::read_json("sandbox bootstrap config", &args.config)?;
    if args.run_as_child_process {
        dispatch_child(&config)
    } else {
        let res = dispatch_parent(args, &config);
        let _file = fs_util::create_file("complete stamp", &config.complete_stamp)?;
        res
    }
}

fn dispatch_parent(args: &BootstrapArgs, config: &SandboxBootstrapConfig) -> eyre::Result<()> {
    fs_util::create_dir_all("output directory", &config.output_dir)?;

    let stdout_path = config.output_dir.join("bootstrap.stdout.txt");
    let stderr_path = config.output_dir.join("bootstrap.stderr.txt");
    let status_path = config.output_dir.join("bootstrap.status.txt");

    let stdout_file = fs_util::create_file("bootstrap stdout", &stdout_path)?;
    let stderr_file = fs_util::create_file("bootstrap stderr", &stderr_path)?;

    let mut cmd = Command::new(&config.xtask_exe);
    let status = cmd
        .arg("bootstrap")
        .arg("--config")
        .arg(&args.config)
        .arg("--run-as-child-process")
        .stdout(stdout_file)
        .stderr(stderr_file)
        .status()
        .wrap_err("failed to spawn or wait for bootstrap process")?;

    fs_util::write("bootstrap status", &status_path, status.to_string())?;

    if !status.success() {
        bail!("bootstrap child process failed with status {status}");
    }

    Ok(())
}

fn dispatch_child(config: &SandboxBootstrapConfig) -> eyre::Result<()> {
    let kind = config.action.to_kind();
    let (report_json, (res, report)) = match &config.action {
        SandboxAction::Noop => return Ok(()),
        SandboxAction::RunTests {
            test_exes,
            report_json,
        } => (
            report_json,
            RunReport::capture(config.run_id, kind, |exec_results| {
                run_test(test_exes, &config.output_dir, exec_results)
            }),
        ),
        SandboxAction::RunScenario {
            scenario,
            report_json,
        } => (
            report_json,
            RunReport::capture(config.run_id, kind, |exec_results| {
                let params = scenario::ScenarioParameters {
                    foton_exe: config.foton_exe.clone(),
                    output_dir: config.output_dir.clone(),
                    run_id: config.run_id,
                };
                scenario::run(*scenario, &params, exec_results)
            }),
        ),
    };
    fs_util::write_json("run report", report_json, &report)?;
    res
}

fn run_test(
    test_exes: &[Utf8PathBuf],
    output_dir: &Utf8Path,
    exec_results: &mut Vec<ExecResult>,
) -> eyre::Result<()> {
    for test_exe in test_exes {
        let mut cmd = Command::new(test_exe);
        let name = test_exe.file_stem().unwrap_or("test");
        let res = process_util::exec_command(name, output_dir, &mut cmd)?;
        let res = exec_results.push_mut(res);
        if !res.success {
            bail!(
                "test executable `{test_exe}` failed with status {}",
                res.exit_status
            );
        }
    }
    Ok(())
}
