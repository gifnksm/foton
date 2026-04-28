use std::collections::BTreeSet;

use crate::{
    command::{
        InstallError,
        install::{InstallErrorReport, InstallStep},
    },
    db::{BeginInstallResult, PackageDatabase},
    package::{PackageId, PackageManifest, PackageVersion},
    util::reporter::{StepReporter, StepResultErrorExt as _},
};

#[derive(Debug)]
pub(in crate::command::install) struct DbGuard<'a, 'b, 'c, 'd> {
    installation_persisted: bool,
    installation_completed_in_memory: bool,
    reporter: &'a StepReporter<'b, InstallStep<'c>>,
    db: PackageDatabase<'d>,
    pkg_id: PackageId,
}

#[derive(Debug)]
pub(in crate::command::install) enum BeginInstallTxResult<'a, 'b, 'c, 'd> {
    CanInstall(DbGuard<'a, 'b, 'c, 'd>),
    AlreadyInstalled(PackageDatabase<'d>),
    OtherVersionInstalled(PackageDatabase<'d>, PackageVersion),
    PendingInstallFound(PackageDatabase<'d>, BTreeSet<PackageVersion>),
    PendingUninstallFound(PackageDatabase<'d>, BTreeSet<PackageVersion>),
}

pub(in crate::command::install) fn begin_install<'a, 'b, 'c, 'd>(
    reporter: &'a StepReporter<'b, InstallStep<'c>>,
    mut db: PackageDatabase<'d>,
    manifest: &PackageManifest,
) -> Result<BeginInstallTxResult<'a, 'b, 'c, 'd>, InstallError> {
    let pkg_id = manifest.metadata.id();
    match db.begin_install(manifest) {
        BeginInstallResult::CanInstall => {}
        BeginInstallResult::AlreadyInstalled => {
            return Ok(BeginInstallTxResult::AlreadyInstalled(db));
        }
        BeginInstallResult::OtherVersionInstalled(version) => {
            return Ok(BeginInstallTxResult::OtherVersionInstalled(db, version));
        }
        BeginInstallResult::PendingInstallFound(versions) => {
            return Ok(BeginInstallTxResult::PendingInstallFound(db, versions));
        }
        BeginInstallResult::PendingUninstallFound(versions) => {
            return Ok(BeginInstallTxResult::PendingUninstallFound(db, versions));
        }
    }

    save(reporter, &mut db)?;

    Ok(BeginInstallTxResult::CanInstall(DbGuard {
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
