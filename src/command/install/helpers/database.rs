use crate::{
    command::{
        InstallError,
        install::{InstallErrorReport, InstallStep},
    },
    db::{BeginInstallResult, BeginUninstallResult, PackageDatabase},
    package::{self, PackageDirs, PackageId, PackageManifest},
    platform::windows::steps::unregistration,
    util::{
        app_dirs::AppDirs,
        reporter::{StepReporter, StepResultErrorExt as _},
    },
};

#[derive(Debug)]
pub(in crate::command::install) struct DbGuard<'a, 'b, 'c, 'd> {
    installation_persisted: bool,
    installation_completed_in_memory: bool,
    reporter: &'a StepReporter<'b, InstallStep<'c>>,
    db: PackageDatabase<'d>,
    pkg_id: PackageId,
}

pub(in crate::command::install) fn begin_install<'a, 'b, 'c, 'd>(
    reporter: &'a StepReporter<'b, InstallStep<'c>>,
    app_id: &str,
    app_dirs: &AppDirs,
    mut db: PackageDatabase<'d>,
    manifest: &PackageManifest,
) -> Result<Option<DbGuard<'a, 'b, 'c, 'd>>, InstallError> {
    let pkg_id = manifest.metadata.id();
    loop {
        let cleanup_versions = match db.begin_install(manifest) {
            BeginInstallResult::CanInstall => break,
            BeginInstallResult::AlreadyInstalled => {
                reporter.report_info(format_args!("package is already installed, skipping"));
                return Ok(None);
            }
            BeginInstallResult::OtherVersionInstalled(version) => {
                reporter.report_info(format_args!(
                    "another version of the package is already installed (version {version}), skipping"
                ));
                return Ok(None);
            }
            BeginInstallResult::HavePendingInstall(versions) => {
                reporter.report_info(format_args!(
                "pending installation detected, uninstalling following packages before continuing:\n{}",
                versions
                    .iter()
                    .map(|version| format!("- {name}@{version}", name = pkg_id.qualified_name()))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
                versions
            }
            BeginInstallResult::HavePendingUninstall(versions) => {
                reporter.report_info(format_args!(
                "pending uninstallation detected, uninstalling following packages before continuing:\n{}",
                versions
                    .iter()
                    .map(|version| format!("- {name}@{version}", name = pkg_id.qualified_name()))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
                versions
            }
        };

        for version in cleanup_versions {
            let uninstall_pkg_id = PackageId::new(pkg_id.qualified_name().clone(), version);
            match db.begin_uninstall(&uninstall_pkg_id) {
                BeginUninstallResult::CanUninstall => {}
                // `cleanup_versions` comes from the current database snapshot, so the entry
                // should still exist here. Reaching this branch means the in-memory DB state
                // became internally inconsistent. This is a bug in our state management, not a
                // recoverable runtime error, so we intentionally panic instead of continuing.
                BeginUninstallResult::NotFound => unreachable!(),
            }
            save(reporter, &mut db)?;

            unregistration::unregister_package_fonts(reporter, app_id, &uninstall_pkg_id)?;
            let pkg_dirs = PackageDirs::new(app_dirs, &uninstall_pkg_id);
            package::remove_package_dirs(&pkg_dirs)
                .map_err(|source| InstallErrorReport::CleanupRemovePackageDirs {
                    uninstall_pkg_id: uninstall_pkg_id.clone(),
                    install_pkg_id: pkg_id.clone(),
                    source,
                })
                .report_error(reporter)?;

            // `begin_uninstall` succeeded just above, so completing the same uninstall should
            // not fail. If it does, the package database state is internally inconsistent.
            // This is a bug in our state management, and continuing would risk compounding the
            // inconsistency, so we intentionally panic here.
            db.complete_uninstall(&uninstall_pkg_id).unwrap();
            save(reporter, &mut db)?;
        }
    }

    save(reporter, &mut db)?;

    Ok(Some(DbGuard {
        installation_persisted: false,
        installation_completed_in_memory: false,
        reporter,
        db,
        pkg_id,
    }))
}

fn save(
    reporter: &StepReporter<'_, InstallStep<'_>>,
    db: &mut PackageDatabase<'_>,
) -> Result<(), InstallError> {
    db.save()
        .map_err(|source| InstallErrorReport::SaveDatabase { source })
        .report_error(reporter)?;
    Ok(())
}

impl DbGuard<'_, '_, '_, '_> {
    pub(in crate::command::install) fn complete_install(mut self) -> Result<(), InstallError> {
        // This guard only reaches completion after `begin_install` has persisted a pending-install
        // entry for `self.pkg_id`. Failure here indicates an internal DB invariant violation.
        // That is a bug in our state management rather than a recoverable runtime error, so we
        // intentionally panic instead of attempting recovery.
        self.db.complete_install(&self.pkg_id).unwrap();
        self.installation_completed_in_memory = true;
        self.save()?;
        self.installation_persisted = true;
        Ok(())
    }

    fn save(&mut self) -> Result<(), InstallError> {
        save(self.reporter, &mut self.db)
    }
}

impl Drop for DbGuard<'_, '_, '_, '_> {
    fn drop(&mut self) {
        if self.installation_persisted {
            return;
        }

        // If `complete_install()` has already advanced the in-memory DB state to `Installed`, the
        // surrounding install flow is already failing and the package-dir / registration guards
        // have already rolled back the external side effects by the time this guard is dropped. We
        // must not persist that `Installed` state here, or the DB would become inconsistent with
        // the actual system state. We therefore only roll back the DB here while it is still in
        // `PendingInstall`.
        if !self.installation_completed_in_memory {
            self.reporter
                .report_info(format_args!("rolling back database changes..."));

            // Dropping before `complete_install()` means the in-memory DB state is still
            // `PendingInstall`, so rollback via `cancel_install()` is still valid here. Failure
            // indicates an internal DB invariant violation. That is a bug in our state management,
            // and silently continuing would leave the in-memory state corrupted, so we intentionally
            // panic.
            self.db.cancel_install(&self.pkg_id).unwrap();

            // After rolling the in-memory DB state back to `PendingInstall`, we make a best-effort
            // attempt to persist that rolled-back state before dropping the guard.
            let _ = self.save();
        }
    }
}
