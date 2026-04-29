use std::path::PathBuf;

use crate::package::PackageSpec;

/// Install and uninstall fonts from package registries.
#[derive(clap::Parser)]
pub(crate) struct Args {
    #[clap(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, clap::Subcommand)]
pub(crate) enum Command {
    /// Install a package from the specified registry.
    Install(InstallArgs),
    /// Uninstall an installed package.
    Uninstall(UninstallArgs),
    /// List installed packages.
    List(ListArgs),
    /// Show detailed information about a package in the local database.
    Info(InfoArgs),
}

#[derive(Debug, clap::Args)]
pub(crate) struct InstallArgs {
    /// Package registry directory to resolve the package from.
    #[clap(long = "registry", value_name = "PATH")]
    pub(crate) registry_path: PathBuf,
    /// Package specifier: name, qualified name, or package ID.
    #[clap(value_name = "PKG_SPEC")]
    pub(crate) pkg_spec: PackageSpec,
}

#[derive(Debug, clap::Args)]
pub(crate) struct UninstallArgs {
    /// Package specifier: name, qualified name, or package ID.
    #[clap(value_name = "PKG_SPEC")]
    pub(crate) pkg_spec: PackageSpec,
}

#[derive(Debug, clap::Args)]
pub(crate) struct ListArgs {
    /// Include packages in pending-install and pending-uninstall states.
    #[clap(long)]
    pub(crate) show_pending: bool,
}

#[derive(Debug, clap::Args)]
pub(crate) struct InfoArgs {
    /// Package specifier: name, qualified name, or package ID.
    #[clap(value_name = "PKG_SPEC")]
    pub(crate) pkg_spec: PackageSpec,
}
