use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use snafu::Snafu;

use crate::{
    cli::{
        args::InstallArgs,
        context::{ReportContext, RootContext},
        message::BulletList,
        reporter::{
            NeverReport, ReportScope, ReportValue, ResultIteratorExt as _, RootReportScope,
        },
    },
    command::{
        common,
        install::helpers::{BeginInstallTxResult, DbGuard},
    },
    db::PackageDatabase,
    package::{
        Package, PackageDirs, PackageId, PackageManifest, PackageQualifiedName, PackageSpec,
        PackageState, PackageVersion,
    },
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
    #[snafu(display("no valid font files found in package {pkg_id}"))]
    NoValidFonts { pkg_id: PackageId },
    #[snafu(display(
        concat!(
            "multiple versions of package `{pkg_name}` are being installed:\n",
            "{versions}\n",
            "this may cause unexpected behavior; consider installing only one version of the package\n",
        ),
        pkg_name = pkg_name,
        versions = BulletList(versions),
    ))]
    MultipleVersions {
        pkg_name: PackageQualifiedName,
        versions: Vec<PackageVersion>,
    },
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
    let InstallArgs {
        registries,
        pkg_specs,
    } = args;

    let cx = InstallScope::start_with_report(
        cx,
        format_args!("Installing {} package(s)...", pkg_specs.len()),
    );

    let registries = common::steps::resolve_registries_by_id(&cx, registries.as_deref())?;

    let mut db_lock_file = common::steps::open_db_lock_file(&cx)?;
    let db = common::steps::load_database(&cx, &mut db_lock_file)?;
    let db = Arc::new(Mutex::new(db));

    let pkg_specs = filter_installed_packages(&cx, &db.lock().unwrap(), pkg_specs);
    if pkg_specs.is_empty() {
        cx.reporter()
            .report_info(format_args!("no packages need to be installed, skipping"));
        return Ok(());
    }

    let indexes = common::steps::fetch_registries(&cx, &registries)?;

    let manifests = common::steps::resolve_package_specs(&cx, &indexes, &pkg_specs)?;
    let manifests = dedup_and_check_conflicts(&cx, &manifests)?;

    for (db, manifest) in begin_install(&cx, &db, &manifests)? {
        let pkg_id = manifest.metadata.id();
        let pkg_dirs = helpers::create_new_package_dirs(&cx, &pkg_id)?;
        let package = stage_package(&cx, &pkg_dirs, &manifest).await?;

        let registration = steps::register_package_fonts(&cx, &package)?;

        db.complete_install()?;

        pkg_dirs.disarm();
        registration.disarm();
    }

    Ok(())
}

fn filter_installed_packages(
    cx: &ReportContext<InstallScope>,
    db: &PackageDatabase<'_>,
    pkg_specs: &[PackageSpec],
) -> Vec<PackageSpec> {
    let mut filtered = vec![];
    for pkg_spec in pkg_specs {
        let installed_pkgs = db
            .entries_by_spec(pkg_spec)
            .filter(|(state, _)| *state == PackageState::Installed)
            .collect::<Vec<_>>();
        if installed_pkgs.is_empty() {
            filtered.push(pkg_spec.clone());
        } else {
            let installed_pkgs = installed_pkgs
                .into_iter()
                .map(|(_, manifest)| manifest.metadata.id())
                .collect::<Vec<_>>();
            cx.reporter().report_info(format_args!(
                concat!(
                    "package {pkg_spec} matches already installed package(s), skipping\n",
                    "{bl}"
                ),
                pkg_spec = pkg_spec,
                bl = BulletList(&installed_pkgs),
            ));
        }
    }
    filtered
}

fn dedup_and_check_conflicts(
    cx: &ReportContext<InstallScope>,
    manifests: &[Arc<PackageManifest>],
) -> Result<Vec<Arc<PackageManifest>>, InstallError> {
    let mut manifest_by_name: BTreeMap<
        PackageQualifiedName,
        BTreeMap<PackageVersion, Arc<PackageManifest>>,
    > = BTreeMap::new();
    for manifest in manifests {
        let name = manifest.metadata.qualified_name.clone();
        let version = manifest.metadata.version.clone();
        manifest_by_name
            .entry(name)
            .or_default()
            .insert(version, Arc::clone(manifest));
    }
    manifest_by_name
        .iter_mut()
        .filter_map(|(pkg_name, manifests_by_version)| {
            (|| {
                if manifests_by_version.len() > 1 {
                    let versions = manifests_by_version.keys().cloned().collect::<Vec<_>>();
                    return Err(cx
                        .reporter()
                        .report_error(MultipleVersionsSnafu { pkg_name, versions }.build()));
                }
                let manifest = manifests_by_version
                    .pop_first()
                    .map(|(_, manifest)| manifest);
                Ok(manifest)
            })()
            .transpose()
        })
        .collect_to_end()
}

type ManifestWithDbGuard<'db> = (DbGuard<'db, InstallScope>, Arc<PackageManifest>);

fn begin_install<'db>(
    cx: &ReportContext<InstallScope>,
    db: &Arc<Mutex<PackageDatabase<'db>>>,
    manifests: &[Arc<PackageManifest>],
) -> Result<Vec<ManifestWithDbGuard<'db>>, InstallError> {
    let reporter = cx.reporter();
    let mut guards = vec![];
    for manifest in manifests {
        let qualified_name = &manifest.metadata.qualified_name;
        loop {
            let cleanup_versions = match helpers::begin_install(cx, Arc::clone(db), manifest)? {
                BeginInstallTxResult::CanInstall(db) => {
                    guards.push((db, Arc::clone(manifest)));
                    break;
                }
                BeginInstallTxResult::AlreadyInstalled => unreachable!(),
                BeginInstallTxResult::OtherVersionInstalled(version) => {
                    reporter.report_info(format_args!(
                    "another version of the package is already installed (version {version}), skipping"
                ));
                    break;
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
    Ok(guards)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        command::common,
        db::BeginInstallResult,
        util::testing::{self, TempdirContext},
    };

    #[test]
    fn filter_installed_packages_skips_name_when_matching_package_is_installed() {
        let cx = TempdirContext::new();
        let cx = InstallScope::start(&cx);
        let mut db_lock_file = common::steps::open_db_lock_file(&cx).unwrap();
        let mut db = common::steps::load_database(&cx, &mut db_lock_file).unwrap();

        let manifest = testing::make_manifest("other-namespace", "example-font", "1.0.0");
        let pkg_id = manifest.metadata.id();
        assert!(matches!(
            db.begin_install(&manifest).unwrap(),
            BeginInstallResult::CanInstall
        ));
        db.complete_install(&pkg_id).unwrap();

        let spec = "example-font".parse::<PackageSpec>().unwrap();
        let filtered = filter_installed_packages(&cx, &db, &[spec]);

        assert!(filtered.is_empty());
    }

    #[test]
    fn filter_installed_packages_keeps_pending_install() {
        let cx = TempdirContext::new();
        let cx = InstallScope::start(&cx);
        let mut db_lock_file = common::steps::open_db_lock_file(&cx).unwrap();
        let mut db = common::steps::load_database(&cx, &mut db_lock_file).unwrap();

        let manifest = testing::make_manifest("example-namespace", "example-font", "1.0.0");
        assert!(matches!(
            db.begin_install(&manifest).unwrap(),
            BeginInstallResult::CanInstall
        ));

        let spec = "example-font".parse::<PackageSpec>().unwrap();
        let filtered = filter_installed_packages(&cx, &db, &[spec]);

        assert_eq!(
            filtered.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec!["example-font"],
        );
    }

    #[test]
    fn filter_installed_packages_skips_only_installed_specs() {
        let cx = TempdirContext::new();
        let cx = InstallScope::start(&cx);
        let mut db_lock_file = common::steps::open_db_lock_file(&cx).unwrap();
        let mut db = common::steps::load_database(&cx, &mut db_lock_file).unwrap();

        let installed_manifest =
            testing::make_manifest("example-namespace", "installed-font", "1.0.0");
        let installed_pkg_id = installed_manifest.metadata.id();
        assert!(matches!(
            db.begin_install(&installed_manifest).unwrap(),
            BeginInstallResult::CanInstall
        ));
        db.complete_install(&installed_pkg_id).unwrap();

        let specs = vec![
            "installed-font".parse::<PackageSpec>().unwrap(),
            "missing-font".parse::<PackageSpec>().unwrap(),
        ];
        let filtered = filter_installed_packages(&cx, &db, &specs);

        assert_eq!(
            filtered.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec!["missing-font"],
        );
    }

    #[test]
    fn dedup_and_check_conflicts_reports_multiple_versions_of_same_package() {
        let cx = TempdirContext::new();
        let cx = InstallScope::start(&cx);
        let manifests = vec![
            Arc::new(testing::make_manifest(
                "example-namespace",
                "example-font",
                "0.1.0",
            )),
            Arc::new(testing::make_manifest(
                "example-namespace",
                "example-font",
                "0.2.0",
            )),
        ];

        let err = dedup_and_check_conflicts(&cx, &manifests).unwrap_err();

        assert!(matches!(err, InstallError::Failed));
    }
}
