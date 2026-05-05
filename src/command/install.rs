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
            NeverReport, OperationError, ReportScope, ReportValue, ResultIteratorExt as _,
            RootReportScope,
        },
    },
    command::common,
    db::{Installability, PackageDatabase},
    engine,
    package::{
        PackageId, PackageManifest, PackageQualifiedName, PackageSpec, PackageState, PackageVersion,
    },
};

#[derive(Debug)]
struct InstallScope {}

impl ReportScope for InstallScope {
    type WarnReportValue = NeverReport;
    type ErrorReportValue = InstallErrorReport;
    type Error = InstallError;
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
    #[snafu(display("operation cancelled"))]
    Cancelled,
}

impl OperationError for InstallError {
    fn failed() -> Self {
        Self::Failed
    }

    fn cancelled() -> Self {
        Self::Cancelled
    }
}

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
    let manifests = cleanup_before_install(&cx, &db, &manifests)?;

    for manifest in manifests {
        engine::execute_install(&cx, &db, &manifest).await?;
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

fn cleanup_before_install(
    cx: &ReportContext<InstallScope>,
    db: &Arc<Mutex<PackageDatabase<'_>>>,
    manifests: &[Arc<PackageManifest>],
) -> Result<Vec<Arc<PackageManifest>>, InstallError> {
    let reporter = cx.reporter();
    let mut install_manifests = vec![];
    for manifest in manifests {
        let qualified_name = &manifest.metadata.qualified_name;
        loop {
            let cleanup_versions = match db.lock().unwrap().check_installability(manifest) {
                Installability::Installable => {
                    install_manifests.push(Arc::clone(manifest));
                    break;
                }
                Installability::AlreadyInstalled => unreachable!(),
                Installability::OtherVersionInstalled(version) => {
                    reporter.report_info(format_args!(
                    "another version of the package is already installed (version {version}), skipping"
                ));
                    break;
                }
                Installability::PendingInstallFound(versions) => {
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
                Installability::PendingUninstallFound(versions) => {
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
                engine::execute_uninstall(cx, db, &uninstall_pkg_id)?;
            }
        }
    }
    Ok(install_manifests)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        command::common,
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
        db.begin_install(&manifest).unwrap();
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
        db.begin_install(&manifest).unwrap();

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
        db.begin_install(&installed_manifest).unwrap();
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

    #[test]
    fn cleanup_before_install_skips_manifest_when_other_version_is_installed() {
        let cx = TempdirContext::new();
        let cx = InstallScope::start(&cx);
        let mut db_lock_file = common::steps::open_db_lock_file(&cx).unwrap();
        let mut db = common::steps::load_database(&cx, &mut db_lock_file).unwrap();

        let installed_manifest =
            testing::make_manifest("example-namespace", "example-font", "1.0.0");
        let installed_pkg_id = installed_manifest.metadata.id();
        db.begin_install(&installed_manifest).unwrap();
        db.complete_install(&installed_pkg_id).unwrap();

        let requested_manifest = Arc::new(testing::make_manifest(
            "example-namespace",
            "example-font",
            "2.0.0",
        ));
        let db = Arc::new(Mutex::new(db));

        let install_manifests =
            cleanup_before_install(&cx, &db, &[Arc::clone(&requested_manifest)]).unwrap();

        assert!(install_manifests.is_empty());
        assert_eq!(
            db.lock()
                .unwrap()
                .entry_by_id(&installed_pkg_id)
                .map(|(state, manifest)| (state, manifest.metadata.id())),
            Some((PackageState::Installed, installed_pkg_id)),
        );
    }
}
