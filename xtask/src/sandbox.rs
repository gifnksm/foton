use std::{
    fmt,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use color_eyre::eyre;
use wsbx::{SandboxConfig, config::MappedFolder};

use crate::{fs_util, scenario::Scenario};

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
struct Dirs {
    base_dir: PathBuf,
    bin_dir: PathBuf,
    foton_exe: PathBuf,
    xtask_exe: PathBuf,
    output_dir: PathBuf,
}

impl Dirs {
    fn new(base_dir: PathBuf) -> Self {
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

    fn new_host(target_dir: &Path, scenario: &Scenario, run_id: RunId) -> Self {
        Dirs::new(target_dir.join(format!(r"windows-sandbox\scenarios\{scenario}\{run_id}")))
    }

    fn new_sandbox() -> Self {
        Dirs::new(PathBuf::from(r"C:\sandbox\"))
    }
}

fn generate_config(scenario: &Scenario) -> eyre::Result<()> {
    let run_id = RunId::new();
    let host_target_dir = crate::host_target_dir()?;
    let host_dirs = Dirs::new_host(&host_target_dir, scenario, run_id);

    fs_util::create_dir_all("base", &host_dirs.base_dir)?;
    fs_util::create_dir_all("binary", &host_dirs.bin_dir)?;
    fs_util::create_dir_all("output", &host_dirs.output_dir)?;
    fs_util::copy(
        "xtask.exe",
        host_target_dir.join(r"debug\xtask.exe"),
        &host_dirs.xtask_exe,
    )?;
    fs_util::copy(
        "foton.exe",
        host_target_dir.join(r"debug\foton.exe"),
        &host_dirs.foton_exe,
    )?;

    let sandbox_dirs = Dirs::new_sandbox();
    let logon_command = format!(
        r"{} scenario run --scenario {} --foton-exe {} --output-dir {}",
        sandbox_dirs.xtask_exe.display(),
        scenario,
        sandbox_dirs.foton_exe.display(),
        sandbox_dirs.output_dir.display()
    );

    let config_path = host_dirs.base_dir.join("sandbox.wsb");
    let config = SandboxConfig::new()
        .mapped_folder(
            MappedFolder::new(&host_dirs.bin_dir)
                .sandbox_folder(&sandbox_dirs.bin_dir)
                .read_only(true),
        )
        .mapped_folder(
            MappedFolder::new(&host_dirs.output_dir)
                .sandbox_folder(&sandbox_dirs.output_dir)
                .read_only(false),
        )
        .logon_command(logon_command)
        .to_pretty_os_string();

    fs_util::write(
        "sandbox config file",
        &config_path,
        config.as_encoded_bytes(),
    )?;

    eprintln!("Generated Windows Sandbox config:");
    eprintln!("  {}", config_path.display());
    eprintln!("Scenario:");
    eprintln!("  {scenario}");
    eprintln!("Run ID:");
    eprintln!("  {run_id}");

    Ok(())
}
