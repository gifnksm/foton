use std::sync::{Arc, Mutex};

use snafu::Snafu;

use crate::{
    cli::{
        args::InstallArgs,
        context::{ReportContext, RootContext},
        message::BulletList,
        reporter::{NeverReport, OperationError, ReportScope, RootReportScope},
    },
    db::{Installability, PackageDatabase},
    engine::{self, ResolvedInstallTarget},
    package::PackageId,
};

#[derive(Debug)]
struct InstallScope {}

impl ReportScope for InstallScope {
    type WarnReportValue = NeverReport;
    type ErrorReportValue = NeverReport;
    type Error = InstallError;
}

impl RootReportScope for InstallScope {
    fn new() -> Self {
        Self {}
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

    let registries = engine::resolve_registries_by_id(&cx, registries.as_deref())?;

    let mut db_lock_file = engine::open_db_lock_file(&cx)?;
    let db = engine::load_database(&cx, &mut db_lock_file)?;

    let targets = engine::resolve_install_targets(&cx, &db, &registries, pkg_specs)?;

    let db = Arc::new(Mutex::new(db));
    let targets = cleanup_before_install(&cx, &db, targets)?;

    if targets.is_empty() {
        cx.reporter().report_info(format_args!(
            "no packages need to be installed, nothing to do"
        ));
        return Ok(());
    }

    cx.reporter().report_info(format_args!(
        "Installing the following packages:\n{}",
        BulletList(
            &targets
                .iter()
                .map(|target| format!("{} ({})", target.manifest.metadata.id(), target.reg_id))
                .collect::<Vec<_>>(),
        )
    ));

    for target in &targets {
        engine::execute_install(&cx, &db, &target.manifest).await?;
    }

    Ok(())
}

fn cleanup_before_install(
    cx: &ReportContext<InstallScope>,
    db: &Arc<Mutex<PackageDatabase<'_>>>,
    targets: Vec<ResolvedInstallTarget>,
) -> Result<Vec<ResolvedInstallTarget>, InstallError> {
    let reporter = cx.reporter();
    let mut install_targets = vec![];
    for target in targets {
        let qualified_name = &target.manifest.metadata.qualified_name;
        loop {
            let cleanup_versions = match db.lock().unwrap().check_installability(&target.manifest) {
                Installability::Installable => {
                    install_targets.push(target);
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
    Ok(install_targets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        package::PackageState,
        registry::RegistryId,
        util::testing::{self, TempdirContext},
    };

    #[test]
    fn cleanup_before_install_skips_manifest_when_other_version_is_installed() {
        let cx = TempdirContext::new();
        let cx = InstallScope::start(&cx);
        let mut db_lock_file = engine::open_db_lock_file(&cx).unwrap();
        let mut db = engine::load_database(&cx, &mut db_lock_file).unwrap();

        let installed_manifest = testing::make_manifest("example-namespace/example-font@1.0.0");
        testing::mark_as_installed(&mut db, &installed_manifest);
        let installed_pkg_id = installed_manifest.metadata.id();

        let requested_manifest = testing::make_manifest("example-namespace/example-font@2.0.0");
        let db = Arc::new(Mutex::new(db));

        let target = ResolvedInstallTarget {
            manifest: requested_manifest,
            reg_id: RegistryId::new("test-registry").unwrap(),
        };

        let install_manifests = cleanup_before_install(&cx, &db, vec![target]).unwrap();

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
