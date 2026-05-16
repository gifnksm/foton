use std::{env, process};

use snafu::Snafu;

use crate::{
    cli::{
        args::{Command, ManifestCommand},
        context::RootContext,
    },
    command::{
        generate_man::GenerateManError, info::InfoError, install::InstallError, list::ListError,
        manifest::CheckManifestError, print_completion::PrintCompletionError, search::SearchError,
        uninstall::UninstallError, update::UpdateError,
    },
};

mod generate_man;
mod info;
mod install;
mod list;
mod manifest;
mod print_completion;
mod search;
mod uninstall;
mod update;

#[derive(Debug, Snafu)]
pub(crate) enum SpecialCommandError {
    #[snafu(transparent)]
    PrintCompletion { source: PrintCompletionError },
    #[snafu(transparent)]
    GenerateMan { source: GenerateManError },
}

#[derive(Debug, Snafu)]
pub(crate) enum CommandError {
    #[snafu(transparent)]
    Install { source: InstallError },
    #[snafu(transparent)]
    Update { source: UpdateError },
    #[snafu(transparent)]
    Uninstall { source: UninstallError },
    #[snafu(transparent)]
    List { source: ListError },
    #[snafu(transparent)]
    Info { source: InfoError },
    #[snafu(transparent)]
    Search { source: SearchError },
    #[snafu(transparent)]
    CheckManifest { source: CheckManifestError },
}

pub(crate) fn run_special_command() -> Result<(), SpecialCommandError> {
    let bin_name = env!("CARGO_BIN_NAME");
    let env_prefix = bin_name.to_uppercase().replace('-', "_");

    if let Ok(shell) = env::var(format!("{env_prefix}_COMPLETE")) {
        print_completion::print_completion(bin_name, &shell)?;
        process::exit(0);
    }

    if let Some(output_dir) = env::var_os(format!("{env_prefix}_GENERATE_MAN_TO")) {
        generate_man::generate_man(&output_dir)?;
        process::exit(0);
    }

    Ok(())
}

pub(crate) async fn run_command(cx: &RootContext, command: Command) -> Result<(), CommandError> {
    match command {
        Command::Install(args) => install::install_package(cx, &args).await?,
        Command::Update(args) => update::update_package(cx, &args).await?,
        Command::Uninstall(args) => uninstall::uninstall_package(cx, &args).await?,
        Command::List(args) => list::list_package(cx, &args)?,
        Command::Info(args) => info::info_package(cx, &args)?,
        Command::Search(args) => search::search_packages(cx, &args)?,
        Command::Manifest(command) => match command {
            ManifestCommand::Check(args) => manifest::check_manifest(cx, &args).await?,
        },
    }
    Ok(())
}
