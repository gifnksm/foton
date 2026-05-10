use std::path::PathBuf;

use crate::{package::PackageSpec, registry::RegistryId, util::text::QueryString};

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
    /// Install packages.
    Install(InstallArgs),
    /// Update installed packages.
    Update(UpdateArgs),
    /// Uninstall installed packages.
    Uninstall(UninstallArgs),
    /// List installed packages.
    List(ListArgs),
    /// Show detailed information about packages.
    Info(InfoArgs),
    /// Search packages from registries.
    Search(SearchArgs),
}

#[derive(Debug, clap::Args)]
pub(crate) struct InstallArgs {
    #[clap(flatten)]
    pub(crate) target: InstallTargetArgs,
}

#[derive(Debug, clap::Args)]
pub(crate) struct InstallTargetArgs {
    /// Install packages defined in the specified manifest files.
    ///
    /// This option can be specified multiple times. It cannot be used together
    /// with `--registry` or `PKG_SPEC`.
    #[clap(long = "manifest", value_name = "MANIFEST", conflicts_with_all = ["registries", "pkg_specs"])]
    pub(crate) manifests: Vec<PathBuf>,
    /// Package registry IDs to resolve the package from.
    ///
    /// Use a comma-separated list such as `--registry local,foton`.
    /// This option is only available when installing by `PKG_SPEC`.
    #[clap(
        long = "registry",
        value_name = "REGISTRY_ID",
        value_delimiter = ',',
        group = "by-spec",
        requires = "pkg_specs"
    )]
    pub(crate) registries: Option<Vec<RegistryId>>,
    /// Package specifiers: name or package ID.
    ///
    /// Required unless `--manifest` is specified. This cannot be used together
    /// with `--manifest`.
    #[clap(value_name = "PKG_SPEC", required_unless_present_any = ["manifests"])]
    pub(crate) pkg_specs: Vec<PackageSpec>,
}

#[derive(Debug)]
pub(crate) enum InstallTargets {
    Manifest {
        manifests: Vec<PathBuf>,
    },
    PackageSpec {
        registries: Option<Vec<RegistryId>>,
        pkg_specs: Vec<PackageSpec>,
    },
}

impl InstallTargetArgs {
    pub(crate) fn to_targets(&self) -> InstallTargets {
        let InstallTargetArgs {
            manifests,
            registries,
            pkg_specs,
        } = self;
        if pkg_specs.is_empty() {
            InstallTargets::Manifest {
                manifests: manifests.clone(),
            }
        } else {
            InstallTargets::PackageSpec {
                registries: registries.clone(),
                pkg_specs: pkg_specs.clone(),
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
    /// Package registry IDs to resolve the package from.
    ///
    /// Use a comma-separated list such as `--registry local,foton`.
    #[clap(long = "registry", value_name = "REGISTRY_ID", value_delimiter = ',')]
    pub(crate) registries: Option<Vec<RegistryId>>,
    /// Package specifiers: name or package ID.
    ///
    /// If not specified, all installed packages will be updated if possible.
    #[clap(value_name = "PKG_SPEC")]
    pub(crate) pkg_specs: Vec<PackageSpec>,
}

#[derive(Debug, clap::Args)]
pub(crate) struct UninstallArgs {
    /// Package specifiers: name or package ID.
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
    /// Package specifiers: name or package ID.
    #[clap(value_name = "PKG_SPEC", required = true)]
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
            &["foton", "install", "--registry", "local"],
        ];
        for case in cases {
            parse_install(case).unwrap_err();
        }
    }
}
