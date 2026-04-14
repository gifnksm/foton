use clap::Parser as _;
use color_eyre::eyre;

use crate::{sandbox::SandboxCommand, scenario::ScenarioCommand};

mod env_util;
mod fs_util;
mod report;
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
