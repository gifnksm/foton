#[cfg(not(windows))]
compile_error!("foton is supported on Windows only.");

use clap::Parser as _;
use color_eyre::eyre;

use crate::{bootstrap::BootstrapArgs, sandbox::SandboxCommand, scenario::ScenarioCommand};

mod bootstrap;
mod report;
mod sandbox;
mod scenario;
mod util;

/// Development helpers for `foton`.
#[derive(clap::Parser)]
struct Args {
    #[clap(subcommand)]
    command: GlobalCommand,
}

#[derive(clap::Subcommand)]
enum GlobalCommand {
    /// Windows Sandbox helpers.
    Sandbox {
        /// Sandbox subcommand to run.
        #[clap(subcommand)]
        command: SandboxCommand,
    },
    /// Scenario helpers.
    Scenario {
        /// Scenario subcommand to run.
        #[clap(subcommand)]
        command: ScenarioCommand,
    },
    /// Internal sandbox bootstrap command.
    ///
    /// This subcommand is used by `cargo xtask sandbox run` inside Windows
    /// Sandbox and is not intended for direct invocation.
    #[clap(hide = true)]
    Bootstrap {
        /// Internal bootstrap arguments.
        #[clap(flatten)]
        command: BootstrapArgs,
    },
}

fn main() -> eyre::Result<()> {
    let Args { command } = Args::parse();

    color_eyre::install()?;
    report::init_panic_hook();

    match command {
        GlobalCommand::Sandbox { command } => sandbox::dispatch(&command)?,
        GlobalCommand::Scenario { command } => scenario::dispatch(&command)?,
        GlobalCommand::Bootstrap { command } => bootstrap::dispatch(&command)?,
    }

    Ok(())
}
