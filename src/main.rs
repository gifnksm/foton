#[cfg(not(windows))]
compile_error!("foton is supported on Windows only.");

use std::{io, process, sync::Arc};

use clap::Parser as _;
use config::ConfigError;
use snafu::{IntoError as _, ResultExt as _, Snafu};
use tokio::signal;

use crate::{
    cli::{
        args::{Args, GlobalArgs},
        context::RootContext,
        message,
        reporter::{Report, RootReporter},
    },
    command::{CommandError, SpecialCommandError},
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
    command::run_special_command()?;

    let Args {
        global_args,
        command,
    } = Args::parse();
    let cx = init_context(global_args).context(InitializationSnafu)?;
    start_task(&cx, async move |cx| command::run_command(cx, command).await)?;

    Ok(())
}

fn init_context(global_args: GlobalArgs) -> Result<RootContext, InitializationError> {
    let app_dirs = AppDirs::from_directories()?;
    let reporter = RootReporter::message_reporter(global_args.warnings_as_errors);
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
                reporter.report(Report::notice(format_args!(
                    "cancellation requested, shutting down..."
                )));
                cancel_token.cancel();
            }
        });
        fut(cx).await.map_err(Into::into)
    })
}
