#[cfg(not(windows))]
compile_error!("foton is supported on Windows only.");

use std::{env, io, process, str::FromStr as _};

use clap::{CommandFactory as _, Parser as _};
use clap_complete::{Generator, Shell};
use color_eyre::eyre::{self, WrapErr as _};
use reqwest::Url;
use semver::Version;

use crate::{
    package::{PackageId, PackageSpec},
    platform::windows,
    util::{app_dirs::AppDirs, hash::Sha256Digest},
};

mod cli;
mod install;
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

    if smoke_test {
        run_smoke_test(APP_ID, &app_dirs)?;
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

fn run_smoke_test(app_id: &str, app_dirs: &AppDirs) -> eyre::Result<()> {
    let pkg_id = PackageId::new("hackgen", Version::new(2, 10, 0))
        .wrap_err("failed to create package ID")?;
    let spec = PackageSpec {
        id: pkg_id,
        url: Url::parse(
            "https://github.com/yuru7/HackGen/releases/download/v2.10.0/HackGen_v2.10.0.zip",
        )
        .wrap_err("failed to parse hackgen download URL")?,
        sha256: Sha256Digest::from_str(
            "sha256:ed182e2a4b95792d94dea7932f6b45280b5ae353651be249d5f6b7867b788db7",
        )?,
    };

    let package = install::install_package(app_id, &spec, app_dirs)?;

    for font in windows::registry::list_registered_package_fonts(app_id, package.id())? {
        println!("{}: {}", font.name(), font.path().display());
    }

    install::uninstall_package(app_id, &package)?;

    Ok(())
}
