use std::collections::BTreeSet;

use crate::{
    cli::{
        args::InstallArgs,
        context::{RootContext, StepContext},
    },
    command::{
        common,
        install::helpers::{BeginInstallTxResult, DbGuard},
    },
    db::{PackageDatabase, PackageDatabaseError},
    package::{Package, PackageDirs, PackageId, PackageManifest},
    registry::{self, FetchRegistryError, RegistryId, RegistrySource},
    util::{
        fs::FsError,
        reporter::{ReportValue, Step, StepResultErrorExt as _},
    },
};

#[derive(Debug)]
struct InstallStep {}

impl Step for InstallStep {
    type WarnReportValue = InstallWarnReport;
    type ErrorReportValue = InstallErrorReport;
    type Error = InstallError;

    fn make_failed(&self) -> Self::Error {
        InstallError::Failed
    }
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
enum InstallWarnReport {
    #[display(
        "failed to remove package directory after install failure: {path}\nmanual cleanup may be required",
        path = pkg_dirs.version_dir().display()
    )]
    RemovePackageDirectoryAfterInstallFailure {
        pkg_dirs: PackageDirs,
        #[error(source)]
        source: FsError,
    },
}

impl From<InstallWarnReport> for ReportValue<'static> {
    fn from(report: InstallWarnReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
enum InstallErrorReport {
    #[display(
        "specified registry `{id}` not found in configuration\navailable registries:\n{registry_ids}",
            registry_ids = registry_ids.iter().map(|id| format!("- {id}")).collect::<Vec<_>>().join("\n")
    )]
    RegistryNotFound {
        id: RegistryId,
        registry_ids: BTreeSet<RegistryId>,
    },
    #[display("no enabled registries found in configuration")]
    NoEnabledRegistries,
    #[display("failed to fetch registry `{id}`")]
    FetchRegistry {
        id: RegistryId,
        #[error(source)]
        source: Box<FetchRegistryError>,
    },
    #[display("failed to save package database")]
    SaveDatabase {
        #[error(source)]
        source: PackageDatabaseError,
    },
    #[display("failed to create package directories for package {pkg_id}")]
    CreatePackageDirs {
        pkg_id: PackageId,
        #[error(source)]
        source: FsError,
    },
    #[display("no valid font files found in package {pkg_id}")]
    NoValidFonts { pkg_id: PackageId },
}

impl From<InstallErrorReport> for ReportValue<'static> {
    fn from(report: InstallErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum InstallError {
    #[display("failed to install package")]
    Failed,
    #[display("install cancelled")]
    Cancelled,
}

mod helpers;
mod steps;

pub(crate) async fn install_package(
    cx: &RootContext,
    args: &InstallArgs,
) -> Result<(), InstallError> {
    let InstallArgs { registry, pkg_spec } = args;

    let cx = cx.with_step(InstallStep {});
    cx.reporter()
        .report_step(format_args!("Installing {pkg_spec}..."));

    let registries = resolve_registries(&cx, registry.as_ref())?;
    if registries.is_empty() {
        return Err(cx
            .reporter()
            .report_error(InstallErrorReport::NoEnabledRegistries));
    }

    let indexes = registries
        .into_iter()
        .map(|(id, source)| {
            registry::fetch_registry(cx.app_dirs(), &id, source).map_err(|source| {
                let source = Box::new(source);
                InstallErrorReport::FetchRegistry { id, source }
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .report_error(cx.reporter())?;

    let manifest = steps::resolve_package(&cx, &indexes, pkg_spec)?;
    let pkg_id = manifest.metadata.id();

    let mut db_lock_file = common::steps::open_db_lock_file(&cx)?;
    let db = common::steps::load_database(&cx, &mut db_lock_file)?;

    let Some(db) = begin_install(&cx, db, &manifest)? else {
        return Ok(());
    };

    let pkg_dirs = helpers::create_new_package_dirs(&cx, &pkg_id)?;
    let package = stage_package(&cx, &pkg_dirs, &manifest).await?;

    let registration = steps::register_package_fonts(&cx, &package)?;

    db.complete_install()?;

    pkg_dirs.disarm();
    registration.disarm();

    Ok(())
}

fn resolve_registries<'a>(
    cx: &'a StepContext<InstallStep>,
    registries: Option<&Vec<RegistryId>>,
) -> Result<Vec<(RegistryId, &'a RegistrySource)>, InstallError> {
    let config_registries = &cx.config().registries;
    let Some(registry_ids) = registries
        .as_ref()
        .map(|registries| registries.iter().cloned().collect::<BTreeSet<_>>())
    else {
        return Ok(config_registries
            .iter()
            .filter(|(_id, registry)| registry.enabled)
            .map(|(id, registry)| (id.clone(), &registry.source))
            .collect());
    };
    let registries: Vec<_> = registry_ids
        .iter()
        .map(|id| {
            config_registries
                .get(id)
                .map(|registry| (id.clone(), &registry.source))
                .ok_or_else(|| {
                    let id = id.clone();
                    let registry_ids = config_registries.keys().cloned().collect();
                    InstallErrorReport::RegistryNotFound { id, registry_ids }
                })
        })
        .collect::<Result<_, _>>()
        .report_error(cx.reporter())?;
    Ok(registries)
}

fn begin_install<'db>(
    cx: &StepContext<InstallStep>,
    mut db: PackageDatabase<'db>,
    manifest: &PackageManifest,
) -> Result<Option<DbGuard<'db>>, InstallError> {
    let reporter = cx.reporter();
    let qualified_name = &manifest.metadata.qualified_name;
    loop {
        let cleanup_versions = match helpers::begin_install(cx, db, manifest)? {
            BeginInstallTxResult::CanInstall(db) => return Ok(Some(db)),
            BeginInstallTxResult::AlreadyInstalled(_db) => {
                reporter.report_info(format_args!("package is already installed, skipping"));
                return Ok(None);
            }
            BeginInstallTxResult::OtherVersionInstalled(_db, version) => {
                reporter.report_info(format_args!(
                    "another version of the package is already installed (version {version}), skipping"
                ));
                return Ok(None);
            }
            BeginInstallTxResult::PendingInstallFound(returned_db, versions) => {
                reporter.report_info(format_args!(
                "pending installation detected, uninstalling following packages before continuing:\n{}",
                versions
                    .iter()
                    .map(|version| format!("- {qualified_name}@{version}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
                db = returned_db;
                versions
            }
            BeginInstallTxResult::PendingUninstallFound(returned_db, versions) => {
                reporter.report_info(format_args!(
                    "pending uninstallation detected, uninstalling following packages before continuing:\n{}",
                    versions
                        .iter()
                        .map(|version| format!("- {qualified_name}@{version}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                ));
                db = returned_db;
                versions
            }
        };

        for version in cleanup_versions {
            let uninstall_pkg_id = PackageId::new(qualified_name.clone(), version);
            common::steps::uninstall_transaction(cx, &mut db, &uninstall_pkg_id)?;
        }
    }
}

async fn stage_package(
    cx: &StepContext<InstallStep>,
    pkg_dirs: &PackageDirs,
    manifest: &PackageManifest,
) -> Result<Package, InstallError> {
    let pkg_id = manifest.metadata.id();
    let reporter = cx.reporter();
    let package_fonts_dir = pkg_dirs.fonts_dir();

    let mut file_paths = vec![];

    for source in &manifest.sources {
        let file = cx
            .cancel_token()
            .run_until_cancelled(steps::download_archive(cx, &pkg_id, source))
            .await
            .unwrap_or(Err(InstallError::Cancelled))?;

        file_paths.extend(steps::extract_archive(
            cx,
            file,
            &source.include,
            package_fonts_dir,
        )?);
    }

    let valid_entries = steps::validate_and_prune_fonts(cx, package_fonts_dir, &file_paths)?;
    if cx.cancel_token().is_cancelled() {
        return Err(InstallError::Cancelled);
    }

    if valid_entries.is_empty() {
        let pkg_id = pkg_id.clone();
        return Err(reporter.report_error(InstallErrorReport::NoValidFonts { pkg_id }));
    }

    reporter.report_info(format_args!(
        "{} valid font(s) found in package",
        valid_entries.len()
    ));

    let package = Package::new(pkg_id.clone(), pkg_dirs.clone(), valid_entries);
    Ok(package)
}
