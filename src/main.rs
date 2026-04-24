#[cfg(not(windows))]
compile_error!("foton is supported on Windows only.");

use std::{env, io, process};

use clap::{CommandFactory as _, Parser as _};
use clap_complete::{Generator, Shell};
use color_eyre::eyre;

use crate::{
    command::InstallConfig,
    platform::windows,
    util::{app_dirs::AppDirs, reporter::Reporter},
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
        print_completion(bin_name, &shell);
        process::exit(0);
    }
    if let Ok(output_dir) = env::var(format!("{env_prefix}_GENERATE_MAN_TO")) {
        generate_man(&output_dir);
        process::exit(0);
    }

    let Args { smoke_test } = Args::parse();
    let app_dirs = AppDirs::from_directories()?;

    let _guard = windows::com::init()?;
    let reporter = Reporter::message_reporter();

    if smoke_test {
        run_smoke_test(&reporter, APP_ID, &app_dirs)?;
    }

    Ok(())
}

fn print_completion(bin_name: &str, shell: &str) {
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
        _ => panic!(
            "error: unknown shell `{shell}`, expected one of `bash`, `elvish`, `fish`, `powershell`, `zsh`, `nushell`"
        ),
    }
}

fn generate_man(output_dir: &str) {
    clap_mangen::generate_to(Args::command(), output_dir).unwrap();
}

fn run_smoke_test(reporter: &Reporter, app_id: &str, app_dirs: &AppDirs) -> eyre::Result<()> {
    let config = InstallConfig {
        max_archive_size_bytes: 100 * 1024 * 1024, // 100 MiB
        max_extracted_files: 50,
        max_extracted_file_size_bytes: 50 * 1024 * 1024, // 50 MiB
    };

    let manifest = toml::from_str(include_str!(
        "../packages/yuru7/hackgen/2.10.0/manifest.toml"
    ))
    .unwrap();
    let package = command::install_package(reporter, app_id, &manifest, app_dirs, &config)?;
    command::uninstall_package(reporter, app_id, &package)?;

    Ok(())
}
