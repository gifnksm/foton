use std::path::PathBuf;

use crate::{package::PackageSpec, registry::RegistryId, util::text::QueryString};

/// Manage font packages from package registries and manifest files.
#[derive(clap::Parser)]
pub(crate) struct Args {
    #[clap(flatten)]
    pub(crate) global_args: GlobalArgs,
    #[clap(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, clap::Args, Default)]
pub(crate) struct GlobalArgs {
    /// Skip interactive confirmation prompts.
    #[clap(long, global = true)]
    pub(crate) no_confirm: bool,
    /// Treat warnings as errors, causing the command to fail if any warning is emitted.
    #[clap(long, global = true)]
    pub(crate) warnings_as_errors: bool,
}

#[derive(Debug, clap::Subcommand)]
pub(crate) enum Command {
    /// Install packages from package registries or manifest files.
    Install(InstallArgs),
    /// Update installed packages from package registries.
    Update(UpdateArgs),
    /// Uninstall packages recorded in the local package database.
    Uninstall(UninstallArgs),
    /// Clean up packages in the local package database that were left by
    /// incomplete installs, uninstalls, or updates.
    ///
    /// Installed packages are not changed by `repair`.
    /// `repair` only performs cleanup; it does not resume an interrupted
    /// install or update.
    Repair(RepairArgs),
    /// List installed packages.
    List(ListArgs),
    /// Show detailed information about packages recorded in the local package database.
    Info(InfoArgs),
    /// Search packages in package registries.
    Search(SearchArgs),
    /// Work with package manifest files.
    #[clap(subcommand)]
    Manifest(ManifestCommand),
}

#[derive(Debug, clap::Args)]
pub(crate) struct InstallArgs {
    #[clap(flatten)]
    pub(crate) target: InstallTargetArgs,
}

#[derive(Debug, clap::Args)]
pub(crate) struct InstallTargetArgs {
    /// Install packages defined in the given manifest files.
    ///
    /// This option can be specified multiple times.
    /// It cannot be used together with `--registry`, `--pre-release` or `<PACKAGE>`.
    #[clap(long = "manifest", value_name = "MANIFEST", conflicts_with_all = ["registries", "pkg_specs", "pre_release"])]
    pub(crate) manifests: Vec<PathBuf>,
    /// Package registry IDs to resolve packages from.
    ///
    /// Use a comma-separated list such as `--registry local,foton`.
    /// This option is only available when installing by `<PACKAGE>`.
    #[clap(
        long = "registry",
        value_name = "REGISTRY_ID",
        value_delimiter = ',',
        group = "by-spec",
        requires = "pkg_specs"
    )]
    pub(crate) registries: Option<Vec<RegistryId>>,
    /// Package names, optionally with an exact version as `<package-name>@<version>`.
    ///
    /// Required unless `--manifest` is specified.
    /// This cannot be used together with `--manifest`.
    #[clap(value_name = "PACKAGE", required_unless_present_any = ["manifests"])]
    pub(crate) pkg_specs: Vec<PackageSpec>,
    /// Allow installing pre-release versions when resolving packages from registries.
    ///
    /// Without this option, versions with a suffix such as `1.2.3-rc-1` are ignored
    /// unless an exact version is specified.
    /// This cannot be used together with `--manifest`.
    #[clap(long)]
    pub(crate) pre_release: bool,
}

#[derive(Debug)]
pub(crate) enum InstallTargets {
    Manifest {
        manifests: Vec<PathBuf>,
    },
    PackageSpec {
        registries: Option<Vec<RegistryId>>,
        pkg_specs: Vec<PackageSpec>,
        pre_release: bool,
    },
}

impl InstallTargetArgs {
    pub(crate) fn to_targets(&self) -> InstallTargets {
        let InstallTargetArgs {
            manifests,
            registries,
            pkg_specs,
            pre_release,
        } = self;
        if pkg_specs.is_empty() {
            InstallTargets::Manifest {
                manifests: manifests.clone(),
            }
        } else {
            InstallTargets::PackageSpec {
                registries: registries.clone(),
                pkg_specs: pkg_specs.clone(),
                pre_release: *pre_release,
            }
        }
    }
}

impl InstallTargets {
    pub(crate) fn len(&self) -> usize {
        match self {
            InstallTargets::Manifest { manifests } => manifests.len(),
            InstallTargets::PackageSpec { pkg_specs, .. } => pkg_specs.len(),
        }
    }
}

#[derive(Debug, clap::Args)]
pub(crate) struct UpdateArgs {
    /// Package registry IDs to resolve packages from.
    ///
    /// Use a comma-separated list such as `--registry local,foton`.
    #[clap(long = "registry", value_name = "REGISTRY_ID", value_delimiter = ',')]
    pub(crate) registries: Option<Vec<RegistryId>>,
    /// Package names, optionally with an exact version as `<package-name>@<version>`.
    ///
    /// If not specified, all installed packages will be updated if possible.
    /// When an exact version is specified, `update` selects the matching installed
    /// package first, then looks for a newer version of the same package name.
    #[clap(value_name = "PACKAGE")]
    pub(crate) pkg_specs: Vec<PackageSpec>,
    /// Allow updating to pre-release versions when resolving packages from registries.
    ///
    /// Without this option, versions with a suffix such as `1.2.3-rc-1` are ignored.
    #[clap(long)]
    pub(crate) pre_release: bool,
}

#[derive(Debug, clap::Args)]
pub(crate) struct UninstallArgs {
    /// Package names, optionally with an exact version as `<package-name>@<version>`.
    #[clap(value_name = "PACKAGE", required = true)]
    pub(crate) pkg_specs: Vec<PackageSpec>,
}

#[derive(Debug, clap::Args)]
pub(crate) struct RepairArgs {
    /// Package names, optionally with an exact version as `<package-name>@<version>`.
    ///
    /// If not specified, every package that needs cleanup will be cleaned up.
    #[clap(value_name = "PACKAGE")]
    pub(crate) pkg_specs: Vec<PackageSpec>,
}

#[derive(Debug, clap::Args)]
pub(crate) struct ListArgs {
    /// Include packages left by incomplete installs, uninstalls, or updates.
    ///
    /// Without this option, only packages in the `installed` state are shown,
    /// and each line includes the activation state.
    /// With this option, each line includes the installation state, and
    /// installed packages also include the activation state.
    #[clap(long)]
    pub(crate) show_incomplete: bool,
}

#[derive(Debug, clap::Args)]
pub(crate) struct InfoArgs {
    /// Package names, optionally with an exact version as `<package-name>@<version>`.
    #[clap(value_name = "PACKAGE", required = true)]
    pub(crate) pkg_specs: Vec<PackageSpec>,
}

#[derive(Debug, clap::Args)]
pub(crate) struct SearchArgs {
    /// Package registry IDs to search.
    ///
    /// Use a comma-separated list such as `--registry local,foton`.
    #[clap(long = "registry", value_name = "REGISTRY_ID", value_delimiter = ',')]
    pub(crate) registries: Option<Vec<RegistryId>>,
    /// Maximum number of matching packages to show.
    #[clap(long, default_value_t = 10, value_parser = parse_positive_usize)]
    pub(crate) limit: usize,
    /// Search query terms.
    ///
    /// All specified query terms must match within the same package metadata field.
    #[clap(value_name = "QUERY", required = true)]
    pub(crate) queries: Vec<QueryString>,
    /// Allow matching pre-release versions when searching packages in registries.
    ///
    /// Without this option, versions with a suffix such as `1.2.3-rc-1` are ignored.
    #[clap(long)]
    pub(crate) pre_release: bool,
}

#[derive(Debug, clap::Subcommand)]
pub(crate) enum ManifestCommand {
    /// Validate a manifest file for installation errors and quality warnings.
    Check(CheckManifestArgs),
}

#[derive(Debug, clap::Args)]
pub(crate) struct CheckManifestArgs {
    /// Path to the manifest file to validate.
    #[clap(value_name = "MANIFEST", required = true)]
    pub(crate) manifest_path: PathBuf,
}

fn parse_positive_usize(s: &str) -> Result<usize, String> {
    let value = s.parse::<usize>().map_err(|e| e.to_string())?;
    if value == 0 {
        return Err("value must be at least 1".to_owned());
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use clap::Parser as _;

    use super::*;

    fn parse_install<I, T>(args: I) -> Result<InstallTargetArgs, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        let args = Args::try_parse_from(args)?;
        let Command::Install(install_args) = args.command else {
            unreachable!("test helper only parses install commands");
        };
        Ok(install_args.target)
    }

    #[test]
    fn install_accepts_valid_argument_combinations() {
        let cases = [
            &["foton", "install", "package1", "package2"][..],
            &["foton", "install", "--pre-release", "package1"][..],
            &[
                "foton",
                "install",
                "--registry",
                "local,foton",
                "package1",
                "package2",
            ][..],
            &[
                "foton",
                "install",
                "--manifest",
                "fonts.toml",
                "--manifest",
                "extras.toml",
            ],
        ];
        for case in cases {
            parse_install(case).unwrap();
        }
    }

    #[test]
    fn install_rejects_invalid_argument_combinations() {
        let cases = [
            &["foton", "install"][..],
            &["foton", "install", "--manifest", "fonts.toml", "package"][..],
            &[
                "foton",
                "install",
                "--manifest",
                "fonts.toml",
                "--registry",
                "local",
            ],
            &[
                "foton",
                "install",
                "--manifest",
                "fonts.toml",
                "--pre-release",
            ][..],
            &["foton", "install", "--registry", "local"],
        ];
        for case in cases {
            parse_install(case).unwrap_err();
        }
    }
}
