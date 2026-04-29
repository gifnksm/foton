use std::collections::BTreeSet;

use crate::{
    cli::context::StepContext,
    command::{
        InstallError,
        install::{InstallErrorReport, InstallStep},
    },
    db::{BeginInstallResult, PackageDatabase},
    package::{PackageId, PackageManifest, PackageVersion},
    util::reporter::StepResultErrorExt as _,
};

#[derive(Debug)]
pub(in crate::command::install) enum BeginInstallTxResult<'db> {
    CanInstall(DbGuard<'db>),
    AlreadyInstalled(PackageDatabase<'db>),
    OtherVersionInstalled(PackageDatabase<'db>, PackageVersion),
    PendingInstallFound(PackageDatabase<'db>, BTreeSet<PackageVersion>),
    PendingUninstallFound(PackageDatabase<'db>, BTreeSet<PackageVersion>),
}

pub(in crate::command::install) fn begin_install<'db>(
    cx: &StepContext<InstallStep>,
    mut db: PackageDatabase<'db>,
    manifest: &PackageManifest,
) -> Result<BeginInstallTxResult<'db>, InstallError> {
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

    save(cx, &mut db)?;

    Ok(BeginInstallTxResult::CanInstall(DbGuard {
        installation_persisted: false,
        installation_completed_in_memory: false,
        cx: cx.clone(),
        db,
        pkg_id: manifest.metadata.id(),
    }))
}

fn save(cx: &StepContext<InstallStep>, db: &mut PackageDatabase<'_>) -> Result<(), InstallError> {
    db.save()
        .map_err(|source| InstallErrorReport::SaveDatabase { source })
        .report_error(cx.reporter())?;
    Ok(())
}

#[derive(Debug)]
pub(in crate::command::install) struct DbGuard<'db> {
    installation_persisted: bool,
    installation_completed_in_memory: bool,
    cx: StepContext<InstallStep>,
    db: PackageDatabase<'db>,
    pkg_id: PackageId,
}

impl DbGuard<'_> {
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
        save(&self.cx, &mut self.db)
    }
}

impl Drop for DbGuard<'_> {
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
            self.cx
                .reporter()
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

#[cfg(test)]
mod tests {
    use crate::{
        command::{common, install::InstallStep},
        package::PackageState,
        util::testing::{self, TempdirContext},
    };

    use super::*;

    fn get_entry_state(db: &PackageDatabase<'_>, pkg_id: &PackageId) -> Option<PackageState> {
        db.entry_by_id(pkg_id).map(|(state, _manifest)| state)
    }

    #[test]
    fn begin_install_persists_pending_install_before_completion() {
        let cx = TempdirContext::new();
        let cx = cx.with_step(InstallStep {});
        let mut db_lock_file = common::steps::open_db_lock_file(&cx).unwrap();
        let db = common::steps::load_database(&cx, &mut db_lock_file).unwrap();
        let manifest = testing::make_manifest("example-namespace", "example-font", "0.1.0");
        let pkg_id = manifest.metadata.id();

        let mut guard = match begin_install(&cx, db, &manifest).unwrap() {
            BeginInstallTxResult::CanInstall(guard) => guard,
            other => panic!("unexpected begin_install result: {other:?}"),
        };

        // Reload from disk while keeping the install guard alive so this assertion verifies
        // `begin_install()` persisted `PendingInstall` before completion, rather than only
        // checking the guard's in-memory DB state.
        guard.db.reload().unwrap();
        assert_eq!(
            get_entry_state(&guard.db, &pkg_id),
            Some(PackageState::PendingInstall)
        );

        drop(guard);
    }

    #[test]
    fn dropping_install_guard_rolls_back_persisted_pending_install() {
        let cx = TempdirContext::new();
        let cx = cx.with_step(InstallStep {});
        let mut db_lock_file = common::steps::open_db_lock_file(&cx).unwrap();
        let manifest = testing::make_manifest("example-namespace", "example-font", "0.1.0");
        let pkg_id = manifest.metadata.id();

        {
            let db = common::steps::load_database(&cx, &mut db_lock_file).unwrap();
            let guard = match begin_install(&cx, db, &manifest).unwrap() {
                BeginInstallTxResult::CanInstall(guard) => guard,
                other => panic!("unexpected begin_install result: {other:?}"),
            };

            drop(guard);
        }

        {
            let db = common::steps::load_database(&cx, &mut db_lock_file).unwrap();
            assert_eq!(get_entry_state(&db, &pkg_id), None);
        }
    }

    #[test]
    fn complete_install_persists_installed_state() {
        let cx = TempdirContext::new();
        let cx = cx.with_step(InstallStep {});
        let mut db_lock_file = common::steps::open_db_lock_file(&cx).unwrap();
        let manifest = testing::make_manifest("example-namespace", "example-font", "0.1.0");
        let pkg_id = manifest.metadata.id();

        {
            let db = common::steps::load_database(&cx, &mut db_lock_file).unwrap();
            let guard = match begin_install(&cx, db, &manifest).unwrap() {
                BeginInstallTxResult::CanInstall(guard) => guard,
                other => panic!("unexpected begin_install result: {other:?}"),
            };

            guard.complete_install().unwrap();
        }

        {
            let db = common::steps::load_database(&cx, &mut db_lock_file).unwrap();
            assert_eq!(get_entry_state(&db, &pkg_id), Some(PackageState::Installed));
        }
    }
}
