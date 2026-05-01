use std::io;

use clap::CommandFactory as _;
use clap_complete::{Generator, Shell};
use snafu::Snafu;

use crate::cli::args::Args;

#[derive(Debug, Snafu)]
pub(crate) enum PrintCompletionError {
    #[snafu(display(
        "unknown shell `{shell}`, expected one of `bash`, `elvish`, `fish`, `powershell`, `zsh`, `nushell`"
    ))]
    UnknownShell { shell: String },
}

pub(crate) fn print_completion(bin_name: &str, shell: &str) -> Result<(), PrintCompletionError> {
    match shell {
        "bash" => print(bin_name, Shell::Bash),
        "elvish" => print(bin_name, Shell::Elvish),
        "fish" => print(bin_name, Shell::Fish),
        "powershell" => print(bin_name, Shell::PowerShell),
        "zsh" => print(bin_name, Shell::Zsh),
        "nushell" => print(bin_name, clap_complete_nushell::Nushell),
        _ => UnknownShellSnafu { shell }.fail()?,
    }
    Ok(())
}

fn print<G>(bin_name: &str, g: G)
where
    G: Generator,
{
    clap_complete::generate(g, &mut Args::command(), bin_name, &mut io::stdout());
}
