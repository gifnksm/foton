use color_eyre::eyre;

use crate::command::{
    bootstrap::BootstrapArgs, sandbox::SandboxCommand, scenario::ScenarioCommand,
};

mod bootstrap;
mod sandbox;
mod scenario;

/// Development helpers for `foton`.
#[derive(clap::Parser)]
pub(crate) struct Args {
    #[clap(subcommand)]
    command: GlobalCommand,
}

#[derive(clap::Subcommand)]
enum GlobalCommand {
    /// Windows Sandbox-related helper commands.
    Sandbox {
        /// Sandbox subcommand to run.
        #[clap(subcommand)]
        command: SandboxCommand,
    },
    /// Commands for running scenarios.
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

pub(crate) fn dispatch(args: &Args) -> eyre::Result<()> {
    match &args.command {
        GlobalCommand::Sandbox { command } => sandbox::dispatch(command)?,
        GlobalCommand::Scenario { command } => scenario::dispatch(command)?,
        GlobalCommand::Bootstrap { command } => bootstrap::dispatch(command)?,
    }

    Ok(())
}
