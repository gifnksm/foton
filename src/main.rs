#[cfg(not(windows))]
compile_error!("foton is supported on Windows only.");

use std::{env, io, process, sync::Arc};

use clap::Parser as _;
use config::ConfigError;
use snafu::{IntoError as _, ResultExt as _, Snafu};
use tokio::signal;

use crate::{
    cli::{
        args::{Args, Command, GlobalArgs},
        context::RootContext,
        message,
        reporter::RootReporter,
    },
    command::{
        GenerateManError, InfoError, InstallError, ListError, PrintCompletionError, UninstallError,
        UpdateError,
    },
    platform::windows::{
        self,
        com::{ComError, ComGuard},
    },
    util::{
        app_dirs::{AppDirs, AppDirsError},
        error::FormatErrorChain as _,
    },
};

mod cli;
mod command;
mod db;
mod engine;
mod package;
mod platform;
mod registry;
mod util;

const APP_ID: &str = "io.github.gifnksm.foton";

#[derive(Debug, Snafu)]
enum FotonError {
    #[snafu(transparent)]
    SpecialCommand { source: SpecialCommandError },
    #[snafu(transparent)]
    Command { source: CommandError },
    #[snafu(display("failed to initialize application"))]
    Initialization { source: InitializationError },
}

#[derive(Debug, Snafu)]
enum SpecialCommandError {
    #[snafu(transparent)]
    PrintCompletion { source: PrintCompletionError },
    #[snafu(transparent)]
    GenerateMan { source: GenerateManError },
}

#[derive(Debug, Snafu)]
enum CommandError {
    #[snafu(transparent)]
    Install { source: InstallError },
    #[snafu(transparent)]
    Update { source: UpdateError },
    #[snafu(transparent)]
    Uninstall { source: UninstallError },
    #[snafu(transparent)]
    List { source: ListError },
    #[snafu(transparent)]
    Info { source: InfoError },
}

#[derive(Debug, Snafu)]
enum InitializationError {
    #[snafu(transparent)]
    AppDirs { source: AppDirsError },
    #[snafu(display("failed to load configuration"))]
    LoadConfig { source: ConfigError },
    #[snafu(transparent)]
    ComInit { source: ComError },
    #[snafu(display("failed to build Tokio runtime"))]
    BuildTokioRuntime { source: io::Error },
    #[snafu(display("failed to listen for ctrl-c event"))]
    ListenCtrlC { source: io::Error },
}

fn main() {
    if let Err(err) = run() {
        message::error!("{}", err.format_error_chain());
        process::exit(1);
    }
}

fn run() -> Result<(), FotonError> {
    run_special_command()?;

    let Args {
        global_args,
        command,
    } = Args::parse();
    let cx = init_context(global_args).context(InitializationSnafu)?;
    start_task(&cx, async move |cx| run_command(cx, command).await)?;

    Ok(())
}

fn init_context(global_args: GlobalArgs) -> Result<RootContext, InitializationError> {
    let app_dirs = AppDirs::from_directories()?;
    let reporter = RootReporter::message_reporter();
    let config = cli::config::load_config(&app_dirs).context(LoadConfigSnafu)?;
    let cx = RootContext::new(
        APP_ID.into(),
        Arc::new(app_dirs),
        Arc::new(global_args),
        Arc::new(config),
        reporter,
    );
    Ok(cx)
}

fn start_task<F, T, E>(cx: &RootContext, fut: F) -> Result<T, FotonError>
where
    F: AsyncFnOnce(&RootContext) -> Result<T, E>,
    E: Into<FotonError>,
{
    let _com_guard = windows::com::init()
        .map_err(InitializationError::from)
        .context(InitializationSnafu)?;

    let rt = tokio::runtime::Builder::new_multi_thread()
        .on_thread_start(|| {
            if let Err(err) = windows::com::init().map(ComGuard::disarm) {
                let err = InitializationSnafu.into_error(err.into());
                panic!("{}", err.format_error_chain());
            }
        })
        .on_thread_stop(windows::com::uninit)
        .enable_all()
        .build()
        .context(BuildTokioRuntimeSnafu)
        .context(InitializationSnafu)?;

    rt.block_on(async {
        let mut ctrl_c = signal::windows::ctrl_c()
            .context(ListenCtrlCSnafu)
            .context(InitializationSnafu)?;
        tokio::spawn({
            let cancel_token = cx.cancel_token().clone();
            let reporter = cx.reporter().clone();
            async move {
                ctrl_c.recv().await;
                reporter.report_warn(format_args!("cancellation requested, shutting down..."));
                cancel_token.cancel();
            }
        });
        fut(cx).await.map_err(Into::into)
    })
}

fn run_special_command() -> Result<(), SpecialCommandError> {
    let bin_name = env!("CARGO_BIN_NAME");
    let env_prefix = bin_name.to_uppercase().replace('-', "_");

    if let Ok(shell) = env::var(format!("{env_prefix}_COMPLETE")) {
        command::print_completion(bin_name, &shell)?;
        process::exit(0);
    }

    if let Some(output_dir) = env::var_os(format!("{env_prefix}_GENERATE_MAN_TO")) {
        command::generate_man(&output_dir)?;
        process::exit(0);
    }

    Ok(())
}

async fn run_command(cx: &RootContext, command: Command) -> Result<(), CommandError> {
    match command {
        Command::Install(args) => command::install_package(cx, &args).await?,
        Command::Update(args) => command::update_package(cx, &args).await?,
        Command::Uninstall(args) => command::uninstall_package(cx, &args).await?,
        Command::List(args) => command::list_package(cx, &args)?,
        Command::Info(args) => command::info_package(cx, &args)?,
    }
    Ok(())
}
