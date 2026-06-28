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
    /// Activate installed packages.
    Activate(ActivateArgs),
    /// Deactivate installed packages.
    Deactivate(DeactivateArgs),
    /// Clean up packages in the local package database that were left by
    /// incomplete installs, uninstalls, updates, activations, or deactivations.
    ///
    /// `repair` only performs cleanup; it does not resume an interrupted
    /// install, update, activation, or deactivation.
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
    /// Work with fonts managed by foton.
    #[clap(subcommand)]
    Font(FontCommand),
}

#[derive(Debug, clap::Args)]
pub(crate) struct InstallArgs {
    /// Install packages defined in the given manifest files.
    ///
    /// This option can be specified multiple times.
    #[clap(
        long = "manifest",
        value_name = "MANIFEST",
        required_unless_present_any = ["pkg_specs"],
    )]
    pub(crate) manifests: Vec<PathBuf>,
    /// Package registry IDs to resolve packages from.
    ///
    /// Use a comma-separated list such as `--registry local,foton`.
    /// This option applies only to packages installed by `<PACKAGE>`.
    #[clap(long = "registry", value_name = "REGISTRY_ID", value_delimiter = ',')]
    pub(crate) registries: Option<Vec<RegistryId>>,
    /// Package names, optionally with an exact version as `<package-name>@<version>`.
    ///
    /// At least one `<PACKAGE>` or `--manifest` is required.
    #[clap(value_name = "PACKAGE", required_unless_present_any = ["manifests"])]
    pub(crate) pkg_specs: Vec<PackageSpec>,
    /// Allow installing pre-release versions when resolving packages from registries.
    ///
    /// Without this option, versions with a suffix such as `1.2.3-rc-1` are ignored
    /// unless an exact version is specified.
    /// This option applies only to packages installed by `<PACKAGE>`.
    #[clap(long)]
    pub(crate) pre_release: bool,
    /// Do not activate the installed packages.
    #[clap(long)]
    pub(crate) no_activate: bool,
    /// Exit immediately if the package database is locked by another operation.
    #[clap(long)]
    pub(crate) exit_on_lock: bool,
}

impl InstallArgs {
    pub(crate) fn len(&self) -> usize {
        self.manifests.len() + self.pkg_specs.len()
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
    /// If not specified, `update` selects the latest installed version of each
    /// package name and updates it if possible.
    /// When an exact version is specified, `update` selects that installed
    /// package first, then looks for a newer version of the same package name.
    #[clap(value_name = "PACKAGE")]
    pub(crate) pkg_specs: Vec<PackageSpec>,
    /// Allow updating to pre-release versions when resolving packages from registries.
    ///
    /// Without this option, versions with a suffix such as `1.2.3-rc-1` are ignored.
    #[clap(long)]
    pub(crate) pre_release: bool,
    /// Exit immediately if the package database is locked by another operation.
    #[clap(long)]
    pub(crate) exit_on_lock: bool,
}

#[derive(Debug, clap::Args)]
pub(crate) struct UninstallArgs {
    /// Package names, optionally with an exact version as `<package-name>@<version>`.
    #[clap(value_name = "PACKAGE", required = true)]
    pub(crate) pkg_specs: Vec<PackageSpec>,
    /// Exit immediately if the package database is locked by another operation.
    #[clap(long)]
    pub(crate) exit_on_lock: bool,
}

#[derive(Debug, clap::Args)]
pub(crate) struct ActivateArgs {
    /// Package names, optionally with an exact version as `<package-name>@<version>`.
    #[clap(value_name = "PACKAGE", required = true)]
    pub(crate) pkg_specs: Vec<PackageSpec>,
    /// Exit immediately if the package database is locked by another operation.
    #[clap(long)]
    pub(crate) exit_on_lock: bool,
}

#[derive(Debug, clap::Args)]
pub(crate) struct DeactivateArgs {
    /// Package names, optionally with an exact version as `<package-name>@<version>`.
    #[clap(value_name = "PACKAGE", required = true)]
    pub(crate) pkg_specs: Vec<PackageSpec>,
    /// Exit immediately if the package database is locked by another operation.
    #[clap(long)]
    pub(crate) exit_on_lock: bool,
}

#[derive(Debug, clap::Args)]
pub(crate) struct RepairArgs {
    /// Package names, optionally with an exact version as `<package-name>@<version>`.
    ///
    /// If not specified, every package that needs cleanup will be cleaned up.
    #[clap(value_name = "PACKAGE")]
    pub(crate) pkg_specs: Vec<PackageSpec>,
    /// Exit immediately if the package database is locked by another operation.
    #[clap(long)]
    pub(crate) exit_on_lock: bool,
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
    /// Exit immediately if the package database is locked by another operation.
    #[clap(long)]
    pub(crate) exit_on_lock: bool,
}

#[derive(Debug, clap::Args)]
pub(crate) struct InfoArgs {
    /// Package names, optionally with an exact version as `<package-name>@<version>`.
    #[clap(value_name = "PACKAGE", required = true)]
    pub(crate) pkg_specs: Vec<PackageSpec>,
    /// Exit immediately if the package database is locked by another operation.
    #[clap(long)]
    pub(crate) exit_on_lock: bool,
    /// For installed packages, also show the fonts directory and installed font files.
    #[clap(long)]
    pub(crate) show_files: bool,
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

#[derive(Debug, clap::Subcommand)]
pub(crate) enum FontCommand {
    /// List fonts managed by foton.
    List(ListFontArgs),
}

#[derive(Debug, clap::Args)]
pub(crate) struct ListFontArgs {
    /// Include all system fonts recognized by Windows.
    #[clap(long)]
    pub(crate) show_system_fonts: bool,
    /// Include all user fonts recognized by Windows.
    #[clap(long)]
    pub(crate) show_user_fonts: bool,
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

    fn parse_install<I, T>(args: I) -> Result<InstallArgs, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        let args = Args::try_parse_from(args)?;
        let Command::Install(install_args) = args.command else {
            unreachable!("test helper only parses install commands");
        };
        Ok(install_args)
    }

    #[test]
    fn install_accepts_supported_argument_combinations() {
        let cases: [(&str, &[&str]); 6] = [
            ("package-only", &["foton", "install", "package1"]),
            (
                "package-only with pre-release",
                &["foton", "install", "--pre-release", "package1"],
            ),
            (
                "manifest-only",
                &["foton", "install", "--manifest", "fonts.toml"],
            ),
            (
                "mixed manifest and package",
                &["foton", "install", "--manifest", "fonts.toml", "package1"],
            ),
            (
                "manifest-only with registry option",
                &[
                    "foton",
                    "install",
                    "--manifest",
                    "fonts.toml",
                    "--registry",
                    "local",
                ],
            ),
            (
                "manifest-only with pre-release option",
                &[
                    "foton",
                    "install",
                    "--manifest",
                    "fonts.toml",
                    "--pre-release",
                ],
            ),
        ];

        for (name, case) in cases {
            parse_install(case).unwrap_or_else(|err| panic!("case `{name}` should parse: {err}"));
        }
    }

    #[test]
    fn install_parses_mixed_input_and_install_options() {
        let install_args = parse_install([
            "foton",
            "install",
            "--manifest",
            "fonts.toml",
            "--manifest",
            "extras.toml",
            "--registry",
            "local,foton",
            "--pre-release",
            "--no-activate",
            "--exit-on-lock",
            "package1",
            "package2",
        ])
        .unwrap();

        assert_eq!(
            install_args.manifests,
            vec![PathBuf::from("fonts.toml"), PathBuf::from("extras.toml")],
        );
        assert_eq!(
            install_args.registries,
            Some(vec![RegistryId::new("local").unwrap(), RegistryId::foton()]),
        );
        assert_eq!(
            install_args.pkg_specs,
            vec!["package1".parse().unwrap(), "package2".parse().unwrap()],
        );
        assert!(install_args.pre_release);
        assert!(install_args.no_activate);
        assert!(install_args.exit_on_lock);
        assert_eq!(install_args.len(), 4);
    }

    #[test]
    fn install_rejects_missing_or_incomplete_targets() {
        let cases: [(&str, &[&str]); 2] = [
            ("no target", &["foton", "install"]),
            (
                "registry without any install target",
                &["foton", "install", "--registry", "local"],
            ),
        ];

        for (name, case) in cases {
            assert!(
                parse_install(case).is_err(),
                "case `{name}` should fail to parse"
            );
        }
    }
}
