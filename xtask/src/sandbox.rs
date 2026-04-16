use std::{
    fmt::{self, Display},
    io::BufReader,
    process::{self, Command, Stdio},
    sync::mpsc::{self, RecvTimeoutError},
    time::{Duration, Instant},
};

use cargo_metadata::{
    CrateType, Message,
    camino::{Utf8Path, Utf8PathBuf},
};
use chrono::{DateTime, Utc};
use color_eyre::eyre::{self, OptionExt as _, WrapErr as _, bail, ensure};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher as _};
use wsbx::{SandboxConfig, SandboxEnvironment, config::MappedFolder};

use crate::{
    env_util, fs_util,
    report::{ScenarioOutcome, ScenarioReport},
    scenario::Scenario,
};

#[derive(Debug, Clone, Copy)]
enum SandboxConfigKind {
    Plain,
    Scenario(Scenario),
}

/// Mutually exclusive options that choose what kind of sandbox config to generate.
#[derive(Debug, clap::Args)]
#[group(required = true, multiple = false)]
pub(crate) struct SandboxConfigKindArgs {
    /// Generate a plain sandbox config.
    #[clap(long)]
    plain: bool,
    /// Generate a sandbox config that runs the specified scenario at logon.
    #[clap(long)]
    scenario: Option<Scenario>,
}

impl SandboxConfigKindArgs {
    fn kind(&self) -> SandboxConfigKind {
        if let Some(scenario) = self.scenario {
            return SandboxConfigKind::Scenario(scenario);
        }
        if self.plain {
            return SandboxConfigKind::Plain;
        }
        unreachable!()
    }
}

/// Windows Sandbox-related helper commands.
#[derive(clap::Subcommand)]
pub(crate) enum SandboxCommand {
    /// Generate a Windows Sandbox config file and supporting artifacts.
    GenerateConfig {
        #[command(flatten)]
        kind_args: SandboxConfigKindArgs,
        /// Open the generated `.wsb` file with the default application after generation.
        #[clap(long)]
        open: bool,
    },
    /// Run a scenario in Windows Sandbox and wait for the result.
    Run {
        /// Scenario to execute in Windows Sandbox.
        #[clap(long)]
        scenario: Scenario,
        /// Timeout in seconds while waiting for scenario completion.
        #[clap(long, default_value_t = 60)]
        timeout: u64,
    },
}

pub(crate) fn dispatch(command: &SandboxCommand) -> eyre::Result<()> {
    match command {
        SandboxCommand::GenerateConfig { kind_args, open } => {
            generate_config(kind_args.kind(), *open)
        }
        SandboxCommand::Run { scenario, timeout } => run(*scenario, Duration::from_secs(*timeout)),
    }
}

fn generate_config(kind: SandboxConfigKind, open: bool) -> eyre::Result<()> {
    let run_id = RunId::new();
    let target_dir = env_util::cargo_target_dir()?;
    let host_paths = MappingPaths::host(&target_dir, kind, run_id);
    let sandbox_paths = MappingPaths::sandbox();

    prepare_host_artifacts(&host_paths)?;

    let config_path = host_paths.base_dir.join("sandbox.wsb");
    let mut config = configure_mapped_folders(SandboxConfig::new(), &host_paths, &sandbox_paths);
    if let Some(command) = kind.logon_command(&sandbox_paths) {
        config = config.logon_command(command);
    }
    let config_str = config.to_pretty_os_string();

    fs_util::write(
        "sandbox config file",
        &config_path,
        config_str.as_encoded_bytes(),
    )?;

    eprintln!("Generated Windows Sandbox config.");
    eprintln!("  Config: {config_path}");
    eprintln!("  Kind: {kind}");
    eprintln!("  Run ID: {run_id}");

    if open {
        eprintln!("Starting sandbox with generated config...");
        let _child = Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(config_path)
            .spawn()
            .wrap_err("failed to start sandbox with generated config")?;
    }

    Ok(())
}

fn run(scenario: Scenario, timeout: Duration) -> eyre::Result<()> {
    let run_id = RunId::new();
    let target_dir = env_util::cargo_target_dir()?;
    let host_paths = MappingPaths::host_scenario(&target_dir, scenario, run_id);
    let sandbox_paths = MappingPaths::sandbox();

    prepare_host_artifacts(&host_paths)?;
    let logon_command = format!(
        r"{} scenario run --scenario {} --foton-exe {} --output-dir {} --report {} --complete-stamp {}",
        sandbox_paths.xtask_exe,
        scenario,
        sandbox_paths.foton_exe,
        sandbox_paths.output_dir,
        sandbox_paths.report,
        sandbox_paths.complete_stamp,
    );

    let config = configure_mapped_folders(SandboxConfig::new(), &host_paths, &sandbox_paths)
        .logon_command(logon_command);

    let (_report_watcher, watcher_rx) = create_output_dir_watcher(&host_paths)?;

    eprintln!("Starting sandbox scenario run...");
    eprintln!("  Scenario: {scenario}");
    eprintln!("  Run ID: {run_id}");
    eprintln!("  Output dir: {}", host_paths.output_dir);
    eprintln!("  Timeout: {timeout:?}");

    let mut sandbox = SandboxGuard::start(scenario, run_id, config)?;
    sandbox.connect()?;
    eprintln!("Waiting for scenario completion...");
    wait_for_completion_stamp(&host_paths, &watcher_rx, timeout)?;
    sandbox.stop()?;

    let report: ScenarioReport = fs_util::read_json("report", &host_paths.report)?;

    eprintln!("Scenario completed.");
    eprintln!("  Scenario: {scenario}");
    eprintln!("  Run ID: {run_id}");
    eprintln!("  Output dir: {}", host_paths.output_dir);
    eprintln!("  Report: {}", host_paths.report);

    match report.outcome {
        ScenarioOutcome::Success => eprintln!("  Result: Success"),
        ScenarioOutcome::Failure { error, sources } => {
            eprintln!("  Result: Failure");
            eprintln!("  Error: {error}");
            for source in sources {
                eprintln!("    caused by: {source}");
            }
            eprintln!("Check captured stdout/stderr files in the output directory for details.");
            bail!("scenario failed");
        }
    }

    Ok(())
}

impl Display for SandboxConfigKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SandboxConfigKind::Plain => write!(f, "plain"),
            SandboxConfigKind::Scenario(scenario) => write!(f, "scenario/{scenario}"),
        }
    }
}

impl SandboxConfigKind {
    fn logon_command(self, sandbox_paths: &MappingPaths) -> Option<String> {
        match self {
            SandboxConfigKind::Plain => None,
            SandboxConfigKind::Scenario(scenario) => Some(format!(
                r"{} scenario run --scenario {} --foton-exe {} --output-dir {}",
                sandbox_paths.xtask_exe,
                scenario,
                sandbox_paths.foton_exe,
                sandbox_paths.output_dir
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RunId {
    timestamp: DateTime<Utc>,
    pid: u32,
}

impl Display for RunId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}-{}",
            self.timestamp.format("%Y%m%d-%H%M%S-%3f"),
            self.pid,
        )
    }
}

impl RunId {
    fn new() -> Self {
        Self {
            timestamp: Utc::now(),
            pid: process::id(),
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
    report: Utf8PathBuf,
    complete_stamp: Utf8PathBuf,
}

impl MappingPaths {
    fn new(base_dir: Utf8PathBuf) -> Self {
        let bin_dir = base_dir.join("bin");
        let foton_exe = bin_dir.join("foton.exe");
        let xtask_exe = bin_dir.join("xtask.exe");
        let output_dir = base_dir.join("output");
        let report = output_dir.join("report.json");
        let complete_stamp = output_dir.join("complete.stamp");
        Self {
            base_dir,
            bin_dir,
            foton_exe,
            xtask_exe,
            output_dir,
            report,
            complete_stamp,
        }
    }

    fn host_plain(target_dir: &Utf8Path, run_id: RunId) -> Self {
        Self::new(
            target_dir
                .join("windows-sandbox")
                .join("plain")
                .join(run_id.to_string()),
        )
    }

    fn host_scenario(target_dir: &Utf8Path, scenario: Scenario, run_id: RunId) -> Self {
        Self::new(
            target_dir
                .join("windows-sandbox")
                .join("scenarios")
                .join(scenario.to_string())
                .join(run_id.to_string()),
        )
    }

    fn host(target_dir: &Utf8Path, kind: SandboxConfigKind, run_id: RunId) -> Self {
        match kind {
            SandboxConfigKind::Plain => Self::host_plain(target_dir, run_id),
            SandboxConfigKind::Scenario(scenario) => {
                Self::host_scenario(target_dir, scenario, run_id)
            }
        }
    }

    fn sandbox() -> Self {
        Self::new(Utf8PathBuf::from(r"C:\sandbox\"))
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
    let _bytes = fs_util::copy("xtask.exe", &xtask_exe, &host_paths.xtask_exe)?;
    let _bytes = fs_util::copy("foton.exe", &foton_exe, &host_paths.foton_exe)?;

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

fn create_output_dir_watcher(
    host_paths: &MappingPaths,
) -> eyre::Result<(RecommendedWatcher, mpsc::Receiver<notify::Result<Event>>)> {
    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(tx).wrap_err("failed to create file watcher")?;
    watcher
        .watch(
            host_paths.output_dir.as_std_path(),
            RecursiveMode::NonRecursive,
        )
        .wrap_err_with(|| format!("failed to watch directory: {}", host_paths.output_dir))?;
    Ok((watcher, rx))
}

fn wait_for_completion_stamp(
    host_paths: &MappingPaths,
    watcher_rx: &mpsc::Receiver<notify::Result<Event>>,
    timeout: Duration,
) -> eyre::Result<()> {
    let target_path = &host_paths.complete_stamp;
    if target_path.exists() {
        return Ok(());
    }

    let deadline = Instant::now() + timeout;
    loop {
        let timeout = deadline.saturating_duration_since(Instant::now());
        let res = match watcher_rx.recv_timeout(timeout) {
            Ok(res) => res,
            Err(RecvTimeoutError::Timeout) => {
                bail!("timed out waiting for scenario completion");
            }
            Err(RecvTimeoutError::Disconnected) => {
                bail!("file watcher disconnected while waiting for scenario completion")
            }
        };
        let msg = res.wrap_err("failed to watch file changes")?;
        if msg.kind.is_create() && msg.paths.iter().any(|path| path == target_path) {
            return Ok(());
        }
    }
}

#[derive(Debug)]
struct SandboxGuard {
    scenario: Scenario,
    run_id: RunId,
    sandbox: SandboxEnvironment,
    stopped: bool,
}

impl SandboxGuard {
    fn start(scenario: Scenario, run_id: RunId, config: SandboxConfig) -> eyre::Result<Self> {
        eprintln!("Starting Windows Sandbox...");
        let sandbox = SandboxEnvironment::builder()
            .config(config)
            .start()
            .wrap_err("failed to start sandbox")?;
        eprintln!("Sandbox started. (Sandbox ID {})", sandbox.id());
        let sandbox = Self {
            scenario,
            run_id,
            sandbox,
            stopped: false,
        };
        Ok(sandbox)
    }

    fn connect(&mut self) -> eyre::Result<()> {
        eprintln!("Connecting to sandbox...");
        self.sandbox.connect().wrap_err_with(|| {
            format!(
                "failed to connect to sandbox (Scenario {}, Run ID {}, Sandbox ID {})",
                self.scenario,
                self.run_id,
                self.sandbox.id(),
            )
        })?;
        Ok(())
    }

    fn stop(&mut self) -> eyre::Result<()> {
        if self.stopped {
            return Ok(());
        }

        eprintln!("Stopping sandbox...");
        self.sandbox.stop().wrap_err_with(|| {
            format!(
                "failed to stop sandbox (Scenario {}, Run ID {}, Sandbox ID {})",
                self.scenario,
                self.run_id,
                self.sandbox.id(),
            )
        })?;
        self.stopped = true;
        Ok(())
    }
}

impl Drop for SandboxGuard {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
