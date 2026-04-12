use std::{
    env,
    path::{Path, PathBuf},
};

use clap::Parser as _;
use color_eyre::eyre::{self, OptionExt};

use crate::{sandbox::SandboxCommand, scenario::ScenarioCommand};

mod sandbox;
mod scenario;

#[derive(clap::Parser)]
struct Args {
    #[clap(subcommand)]
    command: GlobalCommand,
}

#[derive(clap::Subcommand)]
enum GlobalCommand {
    Sandbox {
        #[clap(subcommand)]
        command: SandboxCommand,
    },
    Scenario {
        #[clap(subcommand)]
        command: ScenarioCommand,
    },
}

fn main() -> eyre::Result<()> {
    let Args { command } = Args::parse();

    color_eyre::install()?;

    match command {
        GlobalCommand::Sandbox { command } => sandbox::dispatch(&command)?,
        GlobalCommand::Scenario { command } => scenario::dispatch(&command)?,
    }

    Ok(())
}

fn host_repository_root() -> eyre::Result<&'static Path> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_eyre("failed to get repository root")
}

fn host_target_dir() -> eyre::Result<PathBuf> {
    if let Some(target_dir) = env::var_os("CARGO_TARGET_DIR") {
        let mut target_dir = PathBuf::from(target_dir);
        if target_dir.is_relative() {
            target_dir = host_repository_root()?.join(target_dir);
        }
        return Ok(target_dir);
    }
    Ok(host_repository_root()?.join("target"))
}

const SANDBOX_TARGET_DIR: &str = r"C:\sandbox\target";
