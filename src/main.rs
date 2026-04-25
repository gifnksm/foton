#[cfg(not(windows))]
compile_error!("foton is supported on Windows only.");

use std::{env, io, process};

use clap::{CommandFactory as _, Parser as _};
use clap_complete::{Generator, Shell};
use color_eyre::eyre::{self, WrapErr as _, bail, eyre};
use tokio::{runtime::Runtime, signal};
use tokio_util::sync::CancellationToken;

use crate::{
    command::InstallConfig,
    platform::windows::{self, com::ComGuard},
    util::{app_dirs::AppDirs, error::FormatErrorChain as _, reporter::Reporter},
};

mod cli;
mod command;
mod package;
mod platform;
mod util;

const APP_ID: &str = "io.github.gifnksm.foton";

#[derive(clap::Parser)]
struct Args {
    #[clap(long)]
    smoke_test: bool,
}

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
    let reporter = Reporter::message_reporter();
    let _com_guard = windows::com::init().wrap_err("COM initialization failed")?;

    build_tokio_runtime()?.block_on(run(args, app_dirs, reporter))
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

async fn run(args: Args, app_dirs: AppDirs, reporter: Reporter) -> eyre::Result<()> {
    let mut ctrl_c = signal::windows::ctrl_c().wrap_err("failed to listen for ctrl-c event")?;
    let cancel_token = CancellationToken::new();

    tokio::spawn({
        let cancel_token = cancel_token.clone();
        let reporter = reporter.clone();
        async move {
            ctrl_c.recv().await;
            reporter.report_warn(format_args!("cancellation requested, shutting down..."));
            cancel_token.cancel();
        }
    });

    let Args { smoke_test } = args;
    if smoke_test {
        run_smoke_test(&cancel_token, &reporter, APP_ID, &app_dirs).await?;
    }

    Ok(())
}

async fn run_smoke_test(
    cancel_token: &CancellationToken,
    reporter: &Reporter,
    app_id: &str,
    app_dirs: &AppDirs,
) -> eyre::Result<()> {
    let config = InstallConfig {
        max_archive_size_bytes: 100 * 1024 * 1024, // 100 MiB
        max_extracted_files: 50,
        max_extracted_file_size_bytes: 50 * 1024 * 1024, // 50 MiB
    };

    let manifest = toml::from_str(include_str!(
        "../packages/yuru7/hackgen/2.10.0/manifest.toml"
    ))?;
    let package =
        command::install_package(cancel_token, reporter, app_id, &manifest, app_dirs, &config)
            .await?;

    command::uninstall_package(reporter, app_id, &package)?;

    Ok(())
}
