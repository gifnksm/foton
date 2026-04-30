#[cfg(not(windows))]
compile_error!("foton is supported on Windows only.");

use std::{env, io, process, sync::Arc};

use clap::{CommandFactory as _, Parser as _};
use clap_complete::{Generator, Shell};
use color_eyre::eyre::{self, WrapErr as _, bail, eyre};
use tokio::{runtime::Runtime, signal};

use crate::{
    cli::{
        args::{Args, Command},
        config::FotonConfig,
        context::RootContext,
    },
    platform::windows::{self, com::ComGuard},
    util::{app_dirs::AppDirs, error::FormatErrorChain as _, reporter::RootReporter},
};

mod cli;
mod command;
mod db;
mod package;
mod platform;
mod registry;
mod util;

const APP_ID: &str = "io.github.gifnksm.foton";

fn main() -> eyre::Result<()> {
    color_eyre::install()?;

    let bin_name = env!("CARGO_BIN_NAME");
    let env_prefix = bin_name.to_uppercase().replace('-', "_");

    if let Ok(shell) = env::var(format!("{env_prefix}_COMPLETE")) {
        print_completion(bin_name, &shell)?;
        process::exit(0);
    }

    if let Ok(output_dir) = env::var(format!("{env_prefix}_GENERATE_MAN_TO")) {
        generate_man(&output_dir)?;
        process::exit(0);
    }

    let args = Args::parse();

    let app_dirs = AppDirs::from_directories()?;
    let reporter = RootReporter::message_reporter();
    let config =
        cli::config::load_config(&app_dirs).wrap_err("failed to load configuration file")?;
    let _com_guard = windows::com::init().wrap_err("COM initialization failed")?;

    build_tokio_runtime()?.block_on(run(app_dirs, config, args, reporter))
}

fn print_completion(bin_name: &str, shell: &str) -> eyre::Result<()> {
    fn print<G>(bin_name: &str, g: G)
    where
        G: Generator,
    {
        clap_complete::generate(g, &mut Args::command(), bin_name, &mut io::stdout());
    }

    match shell {
        "bash" => print(bin_name, Shell::Bash),
        "elvish" => print(bin_name, Shell::Elvish),
        "fish" => print(bin_name, Shell::Fish),
        "powershell" => print(bin_name, Shell::PowerShell),
        "zsh" => print(bin_name, Shell::Zsh),
        "nushell" => print(bin_name, clap_complete_nushell::Nushell),
        _ => bail!(
            "error: unknown shell `{shell}`, expected one of `bash`, `elvish`, `fish`, `powershell`, `zsh`, `nushell`"
        ),
    }
    Ok(())
}

fn generate_man(output_dir: &str) -> eyre::Result<()> {
    clap_mangen::generate_to(Args::command(), output_dir).wrap_err("failed to generate man")
}

fn build_tokio_runtime() -> eyre::Result<Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .on_thread_start(|| {
            if let Err(err) = windows::com::init().map(ComGuard::disarm) {
                let err = eyre!(err).wrap_err("COM initialization failed");
                panic!("{}", err.format_error_chain());
            }
        })
        .on_thread_stop(windows::com::uninit)
        .enable_all()
        .build()
        .wrap_err("failed to create Tokio runtime")
}

async fn run(
    app_dirs: AppDirs,
    config: FotonConfig,
    args: Args,
    reporter: RootReporter,
) -> eyre::Result<()> {
    let mut ctrl_c = signal::windows::ctrl_c().wrap_err("failed to listen for ctrl-c event")?;
    let cx = RootContext::new(
        APP_ID.into(),
        Arc::new(app_dirs),
        Arc::new(config),
        reporter,
    );

    tokio::spawn({
        let cancel_token = cx.cancel_token().clone();
        let reporter = cx.reporter().clone();
        async move {
            ctrl_c.recv().await;
            reporter.report_warn(format_args!("cancellation requested, shutting down..."));
            cancel_token.cancel();
        }
    });

    let Args { command } = args;

    match command {
        Command::Install(args) => {
            command::install_package(&cx, &args.registry_path, &args.pkg_spec).await?;
        }
        Command::Uninstall(args) => command::uninstall_package(&cx, &args.pkg_spec)?,
        Command::List(args) => command::list_package(&cx, &args)?,
        Command::Info(args) => command::info_package(&cx, &args.pkg_spec)?,
    }

    Ok(())
}
