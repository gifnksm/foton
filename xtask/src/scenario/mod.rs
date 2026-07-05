use std::{process::Command, sync::Arc};

use cargo_metadata::camino::Utf8PathBuf;
use color_eyre::eyre::{self, WrapErr as _, ensure, eyre};
use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator as _;
use tempfile::TempDir;

use crate::{
    report::{ExecResult, ReportContext, RunId, RunKind, RunReport},
    util::{build, env as env_util, fs as fs_util},
};

mod help_check;
mod install_failure;
mod install_uninstall;
mod manifest_check;
mod update;

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    clap::ValueEnum,
    derive_more::Display,
    Serialize,
    Deserialize,
    strum::EnumIter,
    strum::IntoStaticStr,
)]
#[display(rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Scenario {
    HelpCheck,
    InstallUninstall,
    Update,
    ManifestCheck,
    InstallFailure,
}

/// Commands for running scenarios.
#[derive(clap::Subcommand)]
pub(crate) enum ScenarioCommand {
    /// Run a scenario and print a summary of the captured outputs.
    Run {
        /// Scenario to run.
        #[clap(long)]
        scenario: Scenario,
        #[clap(flatten)]
        args: RunArgs,
    },
    /// List the available scenarios.
    List {
        /// Print the scenario list as JSON.
        #[clap(long)]
        json: bool,
    },
}

/// Common arguments for running a scenario.
#[derive(clap::Args)]
pub(crate) struct RunArgs {
    /// Path to the `foton` executable to run inside the scenario. If omitted, `foton` is built automatically.
    #[clap(long)]
    foton_exe: Option<Utf8PathBuf>,
    /// Directory where scenario outputs are written. If omitted, a temporary directory is used.
    #[clap(long)]
    output_dir: Option<Utf8PathBuf>,
}

pub(crate) fn dispatch(command: &ScenarioCommand) -> eyre::Result<()> {
    match command {
        ScenarioCommand::Run { scenario, args } => {
            let (_tempdir_guard, params) = args.build_parameters()?;
            let (res, report) =
                RunReport::capture(params.run_id, RunKind::Scenario(*scenario), |cx| {
                    run(cx, *scenario, &params)
                });
            report.print_summary();
            res
        }
        ScenarioCommand::List { json } => {
            if *json {
                let scenarios = Scenario::iter().map(|s| s.to_string()).collect::<Vec<_>>();
                println!("{}", serde_json::to_string_pretty(&scenarios)?);
            } else {
                for scenario in Scenario::iter() {
                    println!("{scenario}");
                }
            }
            Ok(())
        }
    }
}

impl RunArgs {
    fn build_parameters(&self) -> eyre::Result<(Option<TempDir>, ScenarioParameters)> {
        let foton_exe = if let Some(path) = self.foton_exe.clone() {
            path
        } else {
            build::build_foton_exe()?
        };
        let (tempdir_guard, output_dir) = if let Some(output_dir) = &self.output_dir {
            fs_util::create_dir_all("output directory", output_dir)?;
            (None, output_dir.clone())
        } else {
            let tempdir = TempDir::new().wrap_err("failed to create temporary output directory")?;
            let path = Utf8PathBuf::from_path_buf(tempdir.path().to_owned()).map_err(|path| {
                eyre!(
                    "failed to convert temporary output directory path to UTF-8: {}",
                    path.display()
                )
            })?;
            (Some(tempdir), path)
        };
        let fixture_dir = env_util::fixture_dir()?;
        Ok((
            tempdir_guard,
            ScenarioParameters {
                foton_exe,
                fixture_dir,
                output_dir,
                run_id: RunId::new(),
            },
        ))
    }
}

#[derive(Debug)]
pub(crate) struct ScenarioParameters {
    pub(crate) foton_exe: Utf8PathBuf,
    pub(crate) fixture_dir: Utf8PathBuf,
    pub(crate) output_dir: Utf8PathBuf,
    pub(crate) run_id: RunId,
}

#[derive(Debug)]
struct ScenarioContext<'a> {
    cx: &'a ReportContext,
    params: &'a ScenarioParameters,
}

pub(crate) fn run(
    cx: &ReportContext,
    scenario: Scenario,
    params: &ScenarioParameters,
) -> eyre::Result<()> {
    let cx = ScenarioContext { cx, params };
    match scenario {
        Scenario::HelpCheck => help_check::run(&cx),
        Scenario::InstallUninstall => install_uninstall::run(&cx),
        Scenario::Update => update::run(&cx),
        Scenario::ManifestCheck => manifest_check::run(&cx),
        Scenario::InstallFailure => install_failure::run(&cx),
    }
}

impl ScenarioContext<'_> {
    fn exec_foton<F>(&self, f: F) -> eyre::Result<Arc<ExecResult>>
    where
        F: FnOnce(&mut Command),
    {
        let mut cmd = Command::new(&self.params.foton_exe);
        f(&mut cmd);
        crate::util::process::exec_command(self.cx, "foton", &self.params.output_dir, &mut cmd)
    }

    fn list_packages(&self) -> eyre::Result<Vec<ListPackageEntry>> {
        self.exec_foton(|cmd| {
            cmd.args(["list", "--exit-on-lock", "--format", "jsonl"]);
        })?
        .ensure_success()?
        .deserialize_stdout_as_jsonl()
    }

    fn list_fonts(&self) -> eyre::Result<Vec<ListFontEntry>> {
        self.exec_foton(|cmd| {
            cmd.args(["font", "list", "--exit-on-lock", "--format", "jsonl"]);
        })?
        .ensure_success()?
        .deserialize_stdout_as_jsonl()
    }

    fn list_package_fonts(&self) -> eyre::Result<Vec<ListFontEntry>> {
        let fonts = self
            .list_fonts()?
            .into_iter()
            .filter(|font| font.location.ty.is_package_dir())
            .collect();
        Ok(fonts)
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, derive_more::Display, derive_more::IsVariant, Deserialize,
)]
#[display(rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub(crate) enum InstallationState {
    Installed,
    IncompleteInstall,
    IncompleteUninstall,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, derive_more::Display, derive_more::IsVariant, Deserialize,
)]
#[display(rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ActivationState {
    Active,
    Inactive,
    IncompleteActivation,
    IncompleteDeactivation,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct ListPackageEntry {
    #[serde(rename = "package-id")]
    pkg_id: String,
    installation_state: InstallationState,
    activation_state: ActivationState,
}

impl ListPackageEntry {
    fn name(&self) -> &str {
        let (name, _version) = self.pkg_id.split_once('@').unwrap();
        name
    }

    fn version(&self) -> &str {
        let (_name, version) = self.pkg_id.split_once('@').unwrap();
        version
    }

    #[track_caller]
    fn ensure_name<P>(&self, predicate: P) -> eyre::Result<&Self>
    where
        P: FnOnce(&str) -> bool,
    {
        let name = self.name();
        ensure!(
            predicate(name),
            "package `{}` name did not satisfy the expected condition: `{}`",
            self.pkg_id,
            name,
        );
        Ok(self)
    }

    #[track_caller]
    fn ensure_version<P>(&self, predicate: P) -> eyre::Result<&Self>
    where
        P: FnOnce(&str) -> bool,
    {
        let version = self.version();
        ensure!(
            predicate(version),
            "package `{}` version did not satisfy the expected condition: `{}`",
            self.pkg_id,
            version,
        );
        Ok(self)
    }

    #[track_caller]
    fn ensure_installation_state<P>(&self, predicate: P) -> eyre::Result<&Self>
    where
        P: FnOnce(InstallationState) -> bool,
    {
        ensure!(
            predicate(self.installation_state),
            "package `{}` installation state did not satisfy the expected condition: `{}`",
            self.pkg_id,
            self.installation_state,
        );
        Ok(self)
    }

    #[track_caller]
    fn ensure_activation_state<P>(&self, predicate: P) -> eyre::Result<&Self>
    where
        P: FnOnce(ActivationState) -> bool,
    {
        ensure!(
            predicate(self.activation_state),
            "package `{}` activation state did not satisfy the expected condition: `{}`",
            self.pkg_id,
            self.activation_state,
        );
        Ok(self)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct ListFontEntry {
    location: FontLocation,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, derive_more::Display, derive_more::IsVariant, Deserialize,
)]
#[display(rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case", rename_all_fields = "kebab-case")]
enum FontLocationType {
    PackageDir,
    SystemDir,
    UserDir,
    Unknown,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct FontLocation {
    #[serde(rename = "type")]
    ty: FontLocationType,
    #[serde(rename = "package-id", default)]
    pkg_id: Option<String>,
}
