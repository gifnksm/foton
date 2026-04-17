#[cfg(not(windows))]
compile_error!("foton is supported on Windows only.");

use std::{env, io, path::PathBuf, process};

use clap::{CommandFactory as _, Parser as _};
use clap_complete::{Generator, Shell};
use color_eyre::eyre;

use crate::platform::windows::registry::{self, FontEntry};

mod package;
mod platform;

const APP_ID: &str = "io.github.gifnksm.foton";

#[derive(clap::Parser)]
struct App {}

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

    let _args = App::parse();

    run_test()?;

    Ok(())
}

fn print_completion(bin_name: &str, shell: &str) {
    fn print<G>(bin_name: &str, g: G)
    where
        G: Generator,
    {
        clap_complete::generate(g, &mut App::command(), bin_name, &mut io::stdout());
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
    clap_mangen::generate_to(App::command(), output_dir).unwrap();
}

fn run_test() -> eyre::Result<()> {
    let pkgid = package::PackageId {
        name: "example-package".to_string(),
        version: "0.1.0".to_string(),
    };

    registry::register_package_fonts(
        APP_ID,
        &pkgid,
        &[FontEntry {
            name: "Example Font".to_string(),
            path: PathBuf::from(r"C:\path\to\example-font.ttf"),
        }],
    )?;

    for entry in registry::list_registered_package_fonts(APP_ID, &pkgid)? {
        println!("{}: {}", entry.name, entry.path.display());
    }

    registry::unregister_package_fonts(APP_ID, &pkgid)?;
    Ok(())
}
