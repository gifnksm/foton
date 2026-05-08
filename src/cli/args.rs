use crate::{package::PackageSpec, registry::RegistryId};

/// Install and uninstall fonts from package registries.
#[derive(clap::Parser)]
pub(crate) struct Args {
    #[clap(flatten)]
    pub(crate) global_args: GlobalArgs,
    #[clap(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, clap::Args, Default)]
pub(crate) struct GlobalArgs {
    /// Bypass any and all interactive confirmation prompts.
    #[clap(long, global = true)]
    pub(crate) no_confirm: bool,
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
    /// Package registry IDs to resolve the package from.
    ///
    /// Use a comma-separated list such as `--registry local,foton`.
    #[clap(long = "registry", value_name = "REGISTRY_ID", value_delimiter = ',')]
    pub(crate) registries: Option<Vec<RegistryId>>,
    /// Package specifiers: name, qualified name, or package ID.
    #[clap(value_name = "PKG_SPEC", required = true)]
    pub(crate) pkg_specs: Vec<PackageSpec>,
}

#[derive(Debug, clap::Args)]
pub(crate) struct UninstallArgs {
    /// Package specifiers: name, qualified name, or package ID.
    #[clap(value_name = "PKG_SPEC", required = true)]
    pub(crate) pkg_specs: Vec<PackageSpec>,
}

#[derive(Debug, clap::Args)]
pub(crate) struct ListArgs {
    /// Include packages in pending-install and pending-uninstall states.
    #[clap(long)]
    pub(crate) show_pending: bool,
}

#[derive(Debug, clap::Args)]
pub(crate) struct InfoArgs {
    /// Package specifiers: name, qualified name, or package ID.
    #[clap(value_name = "PKG_SPEC", required = true)]
    pub(crate) pkg_specs: Vec<PackageSpec>,
}
