use std::path::PathBuf;

use crate::{package::PackageSpec, registry::RegistryId, util::text::QueryString};

/// Manage font packages from package registries and manifest files.
#[derive(clap::Parser)]
#[command(version, propagate_version = true)]
pub(crate) struct Args {
    #[clap(flatten)]
    pub(crate) global_args: GlobalArgs,
    #[clap(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, clap::Args, Default)]
#[command(next_help_heading = "Global Options")]
pub(crate) struct GlobalArgs {
    /// Exit immediately if the package database is locked by another operation.
    #[clap(long, global = true)]
    pub(crate) exit_on_lock: bool,
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
    /// Clean up packages left in incomplete states when commands such as
    /// `install` or `activate` do not complete cleanly.
    ///
    /// `repair` only performs cleanup. It does not retry or resume those
    /// commands.
    Repair(RepairArgs),
    /// List packages recorded in the local package database.
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
    /// If not specified, `update` checks installed packages for newer versions.
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
pub(crate) struct ActivateArgs {
    /// Package names, optionally with an exact version as `<package-name>@<version>`.
    #[clap(value_name = "PACKAGE", required = true)]
    pub(crate) pkg_specs: Vec<PackageSpec>,
}

#[derive(Debug, clap::Args)]
pub(crate) struct DeactivateArgs {
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
    /// Package names, optionally with an exact version as `<package-name>@<version>`.
    ///
    /// If not specified, show all packages in the local package database.
    #[clap(value_name = "PACKAGE")]
    pub(crate) pkg_specs: Vec<PackageSpec>,
    /// Select the output format.
    #[clap(long, default_value_t, value_enum)]
    pub(crate) format: ListFormat,
}

#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
pub(crate) enum ListFormat {
    #[default]
    Text,
    Jsonl,
}

#[derive(Debug, clap::Args)]
pub(crate) struct InfoArgs {
    /// Package names, optionally with an exact version as `<package-name>@<version>`.
    #[clap(value_name = "PACKAGE", required = true)]
    pub(crate) pkg_specs: Vec<PackageSpec>,
    /// For installed packages, also include the fonts directory and installed font files.
    #[clap(long)]
    pub(crate) include_files: bool,
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
    /// Validate manifest files for installation errors and quality warnings.
    Check(CheckManifestArgs),
}

#[derive(Debug, clap::Args)]
pub(crate) struct CheckManifestArgs {
    /// Paths to the manifest files to validate.
    #[clap(value_name = "MANIFEST", required = true)]
    pub(crate) manifests: Vec<PathBuf>,
    /// Skip checks that require downloading and examining the source archives or files.
    #[clap(long)]
    pub(crate) no_source_checks: bool,
    /// Treat the given manifest files as belonging to the package registry rooted at this directory.
    #[clap(long, value_name = "REGISTRY_ROOT")]
    pub(crate) registry_root: Option<PathBuf>,
}

#[derive(Debug, clap::Subcommand)]
pub(crate) enum FontCommand {
    /// List fonts managed by foton.
    List(ListFontArgs),
}

#[derive(Debug, clap::Args)]
pub(crate) struct ListFontArgs {
    /// Also include all system fonts recognized by Windows.
    #[clap(long)]
    pub(crate) include_system_fonts: bool,
    /// Also include all user fonts recognized by Windows.
    #[clap(long)]
    pub(crate) include_user_fonts: bool,
    /// Select the output format.
    #[clap(long, default_value_t, value_enum)]
    pub(crate) format: ListFontFormat,
}

#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
pub(crate) enum ListFontFormat {
    #[default]
    Text,
    Jsonl,
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
    fn install_accepts_package_or_manifest_targets() {
        let cases: [(&str, &[&str]); 3] = [
            ("package-only", &["foton", "install", "package1"]),
            (
                "manifest-only",
                &["foton", "install", "--manifest", "fonts.toml"],
            ),
            (
                "mixed manifest and package",
                &["foton", "install", "--manifest", "fonts.toml", "package1"],
            ),
        ];

        for (name, case) in cases {
            parse_install(case).unwrap_or_else(|err| panic!("case `{name}` should parse: {err}"));
        }
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
