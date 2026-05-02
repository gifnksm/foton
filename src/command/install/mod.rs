use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

use snafu::{OptionExt as _, ResultExt as _, Snafu};

use crate::{
    cli::{
        args::InstallArgs,
        context::{ReportContext, RootContext},
        message::BulletList,
        reporter::{
            NeverReport, ReportScope, ReportValue, RootReportScope, ScopeResultErrorExt as _,
        },
    },
    command::{
        common,
        install::helpers::{BeginInstallTxResult, DbGuard},
    },
    db::PackageDatabase,
    package::{Package, PackageDirs, PackageId, PackageManifest},
    registry::{self, FetchRegistryError, RegistryId, RegistrySource},
};

#[derive(Debug)]
struct InstallScope {}

impl ReportScope for InstallScope {
    type WarnReportValue = NeverReport;
    type ErrorReportValue = InstallErrorReport;
    type Error = InstallError;

    fn make_failed(&self) -> Self::Error {
        InstallError::Failed
    }
}

impl RootReportScope for InstallScope {
    fn new() -> Self {
        Self {}
    }
}

#[derive(Debug, Snafu)]
enum InstallErrorReport {
    #[snafu(display(
        concat!(
            "specified registry `{reg_id}` not found in configuration\n",
            "available registries:\n",
            "{registry_ids}",
        ),
        reg_id = reg_id,
        registry_ids = BulletList(registry_ids),
    ))]
    RegistryNotFound {
        reg_id: RegistryId,
        registry_ids: BTreeSet<RegistryId>,
    },
    #[snafu(display("no enabled registries found in configuration"))]
    NoEnabledRegistries,
    #[snafu(display("failed to fetch registry `{id}`"))]
    FetchRegistry {
        id: RegistryId,
        #[snafu(source(from(FetchRegistryError, Box::new)))]
        source: Box<FetchRegistryError>,
    },
    #[snafu(display("no valid font files found in package {pkg_id}"))]
    NoValidFonts { pkg_id: PackageId },
}

impl From<InstallErrorReport> for ReportValue<'static> {
    fn from(report: InstallErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

#[derive(Debug, Snafu)]
pub(crate) enum InstallError {
    #[snafu(display("failed to install package; see previous messages for details"))]
    Failed,
    #[snafu(display("install cancelled"))]
    Cancelled,
}

mod helpers;
mod steps;

pub(crate) async fn install_package(
    cx: &RootContext,
    args: &InstallArgs,
) -> Result<(), InstallError> {
    let InstallArgs { registry, pkg_spec } = args;

    let cx = InstallScope::start_with_report(cx, format_args!("Installing {pkg_spec}..."));

    let registries = resolve_registries(&cx, registry.as_ref())?;
    if registries.is_empty() {
        return Err(cx.reporter().report_error(NoEnabledRegistriesSnafu.build()));
    }

    let indexes = registries
        .into_iter()
        .map(|(id, source)| {
            registry::fetch_registry(cx.app_dirs(), &id, source).context(FetchRegistrySnafu { id })
        })
        .collect::<Result<Vec<_>, _>>()
        .report_error(cx.reporter())?;

    let manifest = steps::resolve_package(&cx, &indexes, pkg_spec)?;
    let pkg_id = manifest.metadata.id();

    let mut db_lock_file = common::steps::open_db_lock_file(&cx)?;
    let db = common::steps::load_database(&cx, &mut db_lock_file)?;
    let db = Arc::new(Mutex::new(db));

    let Some(db) = begin_install(&cx, &db, &manifest)? else {
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
    cx: &'a ReportContext<InstallScope>,
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
        .map(|reg_id| {
            config_registries
                .get(reg_id)
                .map(|registry| (reg_id.clone(), &registry.source))
                .with_context(|| RegistryNotFoundSnafu {
                    reg_id,
                    registry_ids: config_registries.keys().cloned().collect::<BTreeSet<_>>(),
                })
        })
        .collect::<Result<_, _>>()
        .report_error(cx.reporter())?;
    Ok(registries)
}

fn begin_install<'db>(
    cx: &ReportContext<InstallScope>,
    db: &Arc<Mutex<PackageDatabase<'db>>>,
    manifest: &PackageManifest,
) -> Result<Option<DbGuard<'db, InstallScope>>, InstallError> {
    let reporter = cx.reporter();
    let qualified_name = &manifest.metadata.qualified_name;
    loop {
        let cleanup_versions = match helpers::begin_install(cx, Arc::clone(db), manifest)? {
            BeginInstallTxResult::CanInstall(db) => return Ok(Some(db)),
            BeginInstallTxResult::AlreadyInstalled => {
                reporter.report_info(format_args!("package is already installed, skipping"));
                return Ok(None);
            }
            BeginInstallTxResult::OtherVersionInstalled(version) => {
                reporter.report_info(format_args!(
                    "another version of the package is already installed (version {version}), skipping"
                ));
                return Ok(None);
            }
            BeginInstallTxResult::PendingInstallFound(versions) => {
                let bl = BulletList(
                    &versions
                        .iter()
                        .map(|version| format!("{qualified_name}@{version}"))
                        .collect::<Vec<_>>(),
                );
                reporter.report_info(format_args!(
                    concat!(
                        "pending installation detected, uninstalling following packages before continuing:\n",
                        "{bl}",
                    ),
                    bl = bl,
                ));
                versions
            }
            BeginInstallTxResult::PendingUninstallFound(versions) => {
                let bl = BulletList(
                    &versions
                        .iter()
                        .map(|version| format!("{qualified_name}@{version}"))
                        .collect::<Vec<_>>(),
                );
                reporter.report_info(format_args!(
                    concat!(
                        "pending uninstallation detected, uninstalling following packages before continuing:\n",
                        "{bl}",
                    ),
                    bl = bl,
                ));
                versions
            }
        };

        for version in cleanup_versions {
            let uninstall_pkg_id = PackageId::new(qualified_name.clone(), version);
            common::steps::uninstall_transaction(cx, db, &uninstall_pkg_id)?;
        }
    }
}

async fn stage_package(
    cx: &ReportContext<InstallScope>,
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
        return Err(reporter.report_error(NoValidFontsSnafu { pkg_id }.build()));
    }

    reporter.report_info(format_args!(
        "{} valid font(s) found in package",
        valid_entries.len()
    ));

    let package = Package::new(pkg_id.clone(), pkg_dirs.clone(), valid_entries);
    Ok(package)
}
