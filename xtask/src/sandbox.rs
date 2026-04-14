use std::{
    fmt,
    io::BufReader,
    process::{Command, Stdio},
};

use cargo_metadata::{
    CrateType, Message,
    camino::{Utf8Path, Utf8PathBuf},
};
use chrono::{DateTime, Utc};
use color_eyre::eyre::{self, OptionExt as _, WrapErr as _, ensure};
use wsbx::{SandboxConfig, config::MappedFolder};

use crate::{env_util, fs_util, scenario::Scenario};

#[derive(clap::Subcommand)]
pub(crate) enum SandboxCommand {
    GenerateConfig {
        #[clap(long)]
        scenario: Scenario,
    },
}

pub(crate) fn dispatch(command: &SandboxCommand) -> eyre::Result<()> {
    match command {
        SandboxCommand::GenerateConfig { scenario } => generate_config(scenario),
    }
}

fn generate_config(scenario: &Scenario) -> eyre::Result<()> {
    let run_id = RunId::new();
    let target_dir = env_util::cargo_target_dir()?;
    let host_paths = MappingPaths::new_host(&target_dir, scenario, run_id);
    let sandbox_paths = MappingPaths::new_sandbox();

    prepare_host_artifacts(&host_paths)?;

    let logon_command = format!(
        r"{} scenario run --scenario {} --foton-exe {} --output-dir {}",
        sandbox_paths.xtask_exe, scenario, sandbox_paths.foton_exe, sandbox_paths.output_dir
    );

    let config_path = host_paths.base_dir.join("sandbox.wsb");
    let config = configure_mapped_folders(SandboxConfig::new(), &host_paths, &sandbox_paths)
        .logon_command(logon_command)
        .to_pretty_os_string();

    fs_util::write(
        "sandbox config file",
        &config_path,
        config.as_encoded_bytes(),
    )?;

    eprintln!("Generated Windows Sandbox config:");
    eprintln!("  {}", config_path);
    eprintln!("Scenario:");
    eprintln!("  {scenario}");
    eprintln!("Run ID:");
    eprintln!("  {run_id}");

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RunId {
    timestamp: DateTime<Utc>,
}

impl fmt::Display for RunId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.timestamp.format("%Y%m%d-%H%M%S-%3fZ"))
    }
}

impl RunId {
    fn new() -> Self {
        Self {
            timestamp: Utc::now(),
        }
    }
}

#[derive(Debug)]
struct MappingPaths {
    base_dir: Utf8PathBuf,
    bin_dir: Utf8PathBuf,
    foton_exe: Utf8PathBuf,
    xtask_exe: Utf8PathBuf,
    output_dir: Utf8PathBuf,
}

impl MappingPaths {
    fn new(base_dir: Utf8PathBuf) -> Self {
        let bin_dir = base_dir.join("bin");
        let foton_exe = bin_dir.join("foton.exe");
        let xtask_exe = bin_dir.join("xtask.exe");
        let output_dir = base_dir.join("output");
        Self {
            base_dir,
            bin_dir,
            foton_exe,
            xtask_exe,
            output_dir,
        }
    }

    fn new_host(target_dir: &Utf8Path, scenario: &Scenario, run_id: RunId) -> Self {
        MappingPaths::new(
            target_dir.join(format!(r"windows-sandbox\scenarios\{scenario}\{run_id}")),
        )
    }

    fn new_sandbox() -> Self {
        MappingPaths::new(Utf8PathBuf::from(r"C:\sandbox\"))
    }
}

fn build_foton_exe() -> eyre::Result<Utf8PathBuf> {
    let cargo_bin = env_util::cargo_bin()?;

    let mut command = Command::new(cargo_bin)
        .args([
            "build",
            "-p",
            "foton",
            "--message-format=json-render-diagnostics",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .wrap_err("failed to spawn `cargo build -p foton`")?;

    let mut foton_exe = None;
    let reader = BufReader::new(command.stdout.take().unwrap());
    for message in Message::parse_stream(reader) {
        let message = message.wrap_err("failed to parse cargo message")?;
        if let Message::CompilerArtifact(artifact) = message
            && artifact.target.is_bin()
            && artifact.target.crate_types.contains(&CrateType::Bin)
            && artifact.target.name == "foton"
        {
            foton_exe = artifact.executable;
        }
    }

    let status = command.wait().wrap_err("failed to wait for cargo build")?;
    ensure!(status.success(), "cargo build failed with status {status}");

    let foton_exe = foton_exe.ok_or_eyre("failed to find foton executable in cargo output")?;
    Ok(foton_exe)
}

fn prepare_host_artifacts(host_paths: &MappingPaths) -> eyre::Result<()> {
    let xtask_exe = env_util::current_exe()?;
    let foton_exe = build_foton_exe()?;

    fs_util::create_dir_all("base", &host_paths.base_dir)?;
    fs_util::create_dir_all("binary", &host_paths.bin_dir)?;
    fs_util::create_dir_all("output", &host_paths.output_dir)?;
    fs_util::copy("xtask.exe", &xtask_exe, &host_paths.xtask_exe)?;
    fs_util::copy("foton.exe", &foton_exe, &host_paths.foton_exe)?;

    Ok(())
}

fn configure_mapped_folders(
    config: SandboxConfig,
    host_paths: &MappingPaths,
    sandbox_paths: &MappingPaths,
) -> SandboxConfig {
    config
        .mapped_folder(
            MappedFolder::new(&host_paths.bin_dir)
                .sandbox_folder(&sandbox_paths.bin_dir)
                .read_only(true),
        )
        .mapped_folder(
            MappedFolder::new(&host_paths.output_dir)
                .sandbox_folder(&sandbox_paths.output_dir)
                .read_only(false),
        )
}
