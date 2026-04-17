use std::process::Command;

use cargo_metadata::camino::Utf8PathBuf;
use color_eyre::eyre::{self, WrapErr as _, bail};
use serde::{Deserialize, Serialize};

use crate::{
    fs_util,
    scenario::{self, Scenario},
};

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct SandboxBootstrapConfig {
    pub(crate) foton_exe: Utf8PathBuf,
    pub(crate) xtask_exe: Utf8PathBuf,
    pub(crate) output_dir: Utf8PathBuf,
    pub(crate) complete_stamp: Utf8PathBuf,
    pub(crate) action: SandboxAction,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SandboxAction {
    Noop,
    RunScenario {
        scenario: Scenario,
        report: Utf8PathBuf,
    },
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
    match &config.action {
        SandboxAction::Noop => Ok(()),
        SandboxAction::RunScenario { scenario, report } => scenario::run(
            *scenario,
            &scenario::ScenarioParameters {
                foton_exe: config.foton_exe.clone(),
                output_dir: config.output_dir.clone(),
                report: report.clone(),
            },
        ),
    }
}
