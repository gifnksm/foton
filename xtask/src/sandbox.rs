use std::{
    io::BufReader,
    process::{Command, Stdio},
    sync::mpsc::{self, RecvTimeoutError},
    time::{Duration, Instant},
};

use cargo_metadata::{
    CrateType, Message,
    camino::{Utf8Path, Utf8PathBuf},
};
use color_eyre::eyre::{self, OptionExt as _, WrapErr as _, bail, ensure};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher as _};
use wsbx::{SandboxConfig, SandboxEnvironment, config::MappedFolder};

use crate::{
    bootstrap::{SandboxAction, SandboxBootstrapConfig},
    report::{RunId, RunKind, RunReport},
    scenario::Scenario,
    util::{env as env_util, fs as fs_util},
};

/// Mutually exclusive options that choose what kind of sandbox config to generate.
#[derive(Debug, clap::Args)]
#[group(required = true, multiple = false)]
pub(crate) struct SandboxConfigKindArgs {
    /// Generate a plain sandbox config.
    #[clap(long)]
    plain: bool,
    /// Generate a sandbox config that runs the foton tests at logon.
    #[clap(long)]
    test: bool,
    /// Generate a sandbox config that runs the specified scenario at logon.
    #[clap(long)]
    scenario: Option<Scenario>,
}

impl SandboxConfigKindArgs {
    fn kind(&self) -> RunKind {
        if let Some(scenario) = self.scenario {
            return RunKind::Scenario(scenario);
        }
        if self.plain {
            return RunKind::Noop;
        }
        if self.test {
            return RunKind::Test;
        }
        unreachable!()
    }
}

/// Mutually exclusive options that choose what kind of sandbox config to run.
#[derive(Debug, clap::Args)]
#[group(required = true, multiple = false)]
pub(crate) struct SandboxRunKindArgs {
    /// Run the foton tests.
    #[clap(long)]
    test: bool,
    /// Run the specified scenario.
    #[clap(long)]
    scenario: Option<Scenario>,
}

impl SandboxRunKindArgs {
    fn kind(&self) -> RunKind {
        if let Some(scenario) = self.scenario {
            return RunKind::Scenario(scenario);
        }
        if self.test {
            return RunKind::Test;
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
    /// Run tests or a scenario in Windows Sandbox and wait for the result.
    Run {
        #[clap(flatten)]
        kind_args: SandboxRunKindArgs,
        /// Timeout in seconds while waiting for run completion.
        #[clap(long, default_value_t = 60)]
        timeout: u64,
    },
}

pub(crate) fn dispatch(command: &SandboxCommand) -> eyre::Result<()> {
    match command {
        SandboxCommand::GenerateConfig { kind_args, open } => {
            generate_config(kind_args.kind(), *open)
        }
        SandboxCommand::Run { kind_args, timeout } => {
            run(kind_args.kind(), Duration::from_secs(*timeout))
        }
    }
}

fn generate_config(kind: RunKind, open: bool) -> eyre::Result<()> {
    let run_id = RunId::new();
    let target_dir = env_util::cargo_target_dir()?;
    let host_paths = MappingPaths::host(&target_dir, kind, run_id);
    let sandbox_paths = MappingPaths::sandbox();

    prepare_host_artifacts(run_id, kind, &host_paths, &sandbox_paths)?;

    let config_path = host_paths.base_dir.join("sandbox.wsb");
    let config_str = sandbox_config(&host_paths, &sandbox_paths).to_pretty_os_string();

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

fn run(kind: RunKind, timeout: Duration) -> eyre::Result<()> {
    let run_id = RunId::new();
    let target_dir = env_util::cargo_target_dir()?;
    let host_paths = MappingPaths::host(&target_dir, kind, run_id);
    let sandbox_paths = MappingPaths::sandbox();

    prepare_host_artifacts(run_id, kind, &host_paths, &sandbox_paths)?;

    let config = sandbox_config(&host_paths, &sandbox_paths);

    let (_report_watcher, watcher_rx) = create_output_dir_watcher(&host_paths)?;

    eprintln!("Starting sandbox...");
    eprintln!("  Kind: {kind}");
    eprintln!("  Run ID: {run_id}");
    eprintln!("  Output dir: {}", host_paths.output_dir);
    eprintln!("  Timeout: {timeout:?}");

    let mut sandbox = SandboxGuard::start(kind, run_id, config)?;
    sandbox.connect()?;
    eprintln!("Waiting for run completion...");
    wait_for_completion_stamp(&host_paths, &watcher_rx, timeout)?;
    sandbox.stop()?;

    let report: RunReport = fs_util::read_json("report", &host_paths.report_json)?;
    report.print_summary();
    ensure!(report.is_success(), "run failed");

    Ok(())
}

#[derive(Debug)]
struct MappingPaths {
    base_dir: Utf8PathBuf,
    bin_dir: Utf8PathBuf,
    foton_exe: Utf8PathBuf,
    xtask_exe: Utf8PathBuf,
    config_dir: Utf8PathBuf,
    bootstrap_config: Utf8PathBuf,
    output_dir: Utf8PathBuf,
    report_json: Utf8PathBuf,
    complete_stamp: Utf8PathBuf,
}

impl MappingPaths {
    fn new(base_dir: Utf8PathBuf) -> Self {
        let bin_dir = base_dir.join("bin");
        let foton_exe = bin_dir.join("foton.exe");
        let xtask_exe = bin_dir.join("xtask.exe");
        let config_dir = base_dir.join("config");
        let bootstrap_config = config_dir.join("bootstrap.config.json");
        let output_dir = base_dir.join("output");
        let report = output_dir.join(".report.json");
        let complete_stamp = output_dir.join(".complete.stamp");
        Self {
            base_dir,
            bin_dir,
            foton_exe,
            xtask_exe,
            config_dir,
            bootstrap_config,
            output_dir,
            report_json: report,
            complete_stamp,
        }
    }

    fn host_noop(target_dir: &Utf8Path, run_id: RunId) -> Self {
        Self::new(
            target_dir
                .join("windows-sandbox")
                .join("plain")
                .join(run_id.to_string()),
        )
    }

    fn host_test(target_dir: &Utf8Path, run_id: RunId) -> Self {
        Self::new(
            target_dir
                .join("windows-sandbox")
                .join("test")
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

    fn host(target_dir: &Utf8Path, kind: RunKind, run_id: RunId) -> Self {
        match kind {
            RunKind::Noop => Self::host_noop(target_dir, run_id),
            RunKind::Test => Self::host_test(target_dir, run_id),
            RunKind::Scenario(scenario) => Self::host_scenario(target_dir, scenario, run_id),
        }
    }

    fn sandbox() -> Self {
        Self::new(Utf8PathBuf::from(r"C:\sandbox\"))
    }

    fn logon_command(&self) -> String {
        format!(
            r#""{}" bootstrap --config "{}""#,
            self.xtask_exe, self.bootstrap_config,
        )
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
            && let Some(exe) = artifact.executable
        {
            foton_exe = Some(exe);
        }
    }

    let status = command.wait().wrap_err("failed to wait for cargo build")?;
    ensure!(status.success(), "cargo build failed with status {status}");

    let foton_exe = foton_exe.ok_or_eyre("failed to find foton executable in cargo output")?;
    Ok(foton_exe)
}

fn build_test_exes() -> eyre::Result<Vec<Utf8PathBuf>> {
    let cargo_bin = env_util::cargo_bin()?;

    let mut command = Command::new(cargo_bin)
        .args([
            "test",
            "--no-run",
            "-p",
            "foton",
            "--message-format=json-render-diagnostics",
        ])
        .env("FOTON_SANDBOX_TEST", "1")
        .stdout(Stdio::piped())
        .spawn()
        .wrap_err("failed to spawn `cargo test --no-run -p foton`")?;

    let mut test_exes = vec![];
    let reader = BufReader::new(command.stdout.take().unwrap());
    for message in Message::parse_stream(reader) {
        let message = message.wrap_err("failed to parse cargo message")?;
        if let Message::CompilerArtifact(artifact) = message
            && artifact.target.name == "foton"
            && let Some(exe) = artifact.executable
        {
            test_exes.push(exe);
        }
    }

    let status = command.wait().wrap_err("failed to wait for cargo test")?;
    ensure!(status.success(), "cargo test failed with status {status}");

    ensure!(
        !test_exes.is_empty(),
        "cargo test did not produce any test executables for foton"
    );
    Ok(test_exes)
}

fn prepare_host_artifacts(
    run_id: RunId,
    kind: RunKind,
    host_paths: &MappingPaths,
    sandbox_paths: &MappingPaths,
) -> eyre::Result<()> {
    let xtask_exe = env_util::current_exe()?;
    let foton_exe = build_foton_exe()?;

    fs_util::create_dir_all("base", &host_paths.base_dir)?;
    fs_util::create_dir_all("binary", &host_paths.bin_dir)?;
    fs_util::create_dir_all("config", &host_paths.config_dir)?;
    fs_util::create_dir_all("output", &host_paths.output_dir)?;

    let _bytes = fs_util::copy("xtask.exe", &xtask_exe, &host_paths.xtask_exe)?;
    let _bytes = fs_util::copy("foton.exe", &foton_exe, &host_paths.foton_exe)?;

    let action = match kind {
        RunKind::Noop => SandboxAction::Noop,
        RunKind::Test => {
            let host_test_exes = build_test_exes()?;
            let mut sandbox_test_exes = vec![];
            for host_test_exe in host_test_exes {
                let file_name = host_test_exe.file_name().unwrap();
                let sandbox_test_exe = sandbox_paths.bin_dir.join(file_name);
                sandbox_test_exes.push(sandbox_test_exe);
                let _bytes = fs_util::copy(
                    "test executable",
                    &host_test_exe,
                    host_paths.bin_dir.join(file_name),
                )?;
            }
            SandboxAction::RunTests {
                test_exes: sandbox_test_exes,
                report_json: sandbox_paths.report_json.clone(),
            }
        }
        RunKind::Scenario(scenario) => SandboxAction::RunScenario {
            scenario,
            report_json: sandbox_paths.report_json.clone(),
        },
    };
    let bootstrap_config = build_bootstrap_config(run_id, sandbox_paths, action);

    fs_util::write_json(
        "bootstrap config",
        &host_paths.bootstrap_config,
        &bootstrap_config,
    )?;

    Ok(())
}

fn sandbox_config(host_paths: &MappingPaths, sandbox_paths: &MappingPaths) -> SandboxConfig {
    configure_mapped_folders(SandboxConfig::new(), host_paths, sandbox_paths)
        .logon_command(sandbox_paths.logon_command())
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
            MappedFolder::new(&host_paths.config_dir)
                .sandbox_folder(&sandbox_paths.config_dir)
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
    kind: RunKind,
    run_id: RunId,
    sandbox: SandboxEnvironment,
    stopped: bool,
}

impl SandboxGuard {
    fn start(kind: RunKind, run_id: RunId, config: SandboxConfig) -> eyre::Result<Self> {
        eprintln!("Starting Windows Sandbox...");
        let sandbox = SandboxEnvironment::builder()
            .config(config)
            .start()
            .wrap_err("failed to start sandbox")?;
        eprintln!("Sandbox started. (Sandbox ID {})", sandbox.id());
        let sandbox = Self {
            kind,
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
                "failed to connect to sandbox (Kind {}, Run ID {}, Sandbox ID {})",
                self.kind,
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
                "failed to stop sandbox (Kind {}, Run ID {}, Sandbox ID {})",
                self.kind,
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

fn build_bootstrap_config(
    run_id: RunId,
    sandbox_paths: &MappingPaths,
    action: SandboxAction,
) -> SandboxBootstrapConfig {
    SandboxBootstrapConfig {
        foton_exe: sandbox_paths.foton_exe.clone(),
        xtask_exe: sandbox_paths.xtask_exe.clone(),
        output_dir: sandbox_paths.output_dir.clone(),
        complete_stamp: sandbox_paths.complete_stamp.clone(),
        run_id,
        action,
    }
}
