use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

use snafu::{ResultExt as _, Snafu};

use crate::{
    cli::{
        context::ReportContext,
        reporter::{
            NeverReport, ReportScope, ReportValue, ScopeResultErrorExt as _, SubReportScope,
        },
    },
    db::{BeginInstallResult, PackageDatabase, PackageDatabaseError},
    package::{PackageId, PackageManifest, PackageVersion},
};

#[derive(Debug)]
struct BeginInstallScope<S> {
    base_scope: Arc<S>,
}

impl<S> ReportScope for BeginInstallScope<S>
where
    S: ReportScope,
{
    type WarnReportValue = NeverReport;
    type ErrorReportValue = BeginInstallErrorReport;
    type Error = S::Error;

    fn make_failed(&self) -> Self::Error {
        self.base_scope.make_failed()
    }
}

impl<S> SubReportScope<S> for BeginInstallScope<S>
where
    S: ReportScope,
{
    fn new(base_scope: Arc<S>) -> Self {
        Self { base_scope }
    }
}

#[derive(Debug, Snafu)]
#[expect(clippy::enum_variant_names)]
enum BeginInstallErrorReport {
    #[snafu(display("failed to begin install transaction for package {pkg_id}"))]
    BeginInstall {
        pkg_id: PackageId,
        source: PackageDatabaseError,
    },
    #[snafu(display("failed to complete install transaction for package {pkg_id}"))]
    CompleteInstall {
        pkg_id: PackageId,
        source: PackageDatabaseError,
    },
    #[snafu(display(
        "\
        failed to roll back install transaction for package {pkg_id}\n\
        manual database cleanup may be required\
        "
    ))]
    CancelInstall {
        pkg_id: PackageId,
        source: PackageDatabaseError,
    },
}

impl From<BeginInstallErrorReport> for ReportValue<'static> {
    fn from(report: BeginInstallErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

#[derive(Debug)]
pub(in crate::command::install) enum BeginInstallTxResult<'db, S>
where
    S: ReportScope,
{
    CanInstall(DbGuard<'db, S>),
    AlreadyInstalled,
    OtherVersionInstalled(PackageVersion),
    PendingInstallFound(BTreeSet<PackageVersion>),
    PendingUninstallFound(BTreeSet<PackageVersion>),
}

pub(in crate::command::install) fn begin_install<'db, S>(
    cx: &ReportContext<S>,
    db: Arc<Mutex<PackageDatabase<'db>>>,
    manifest: &PackageManifest,
) -> Result<BeginInstallTxResult<'db, S>, S::Error>
where
    S: ReportScope,
{
    let cx = BeginInstallScope::start(cx);
    let pkg_id = manifest.metadata.id();

    {
        let mut db = db.lock().unwrap();

        let res = db
            .begin_install(manifest)
            .context(BeginInstallSnafu { pkg_id: &pkg_id })
            .report_error(cx.reporter())?;

        match res {
            BeginInstallResult::CanInstall => {}
            BeginInstallResult::AlreadyInstalled => {
                return Ok(BeginInstallTxResult::AlreadyInstalled);
            }
            BeginInstallResult::OtherVersionInstalled(version) => {
                return Ok(BeginInstallTxResult::OtherVersionInstalled(version));
            }
            BeginInstallResult::PendingInstallFound(versions) => {
                return Ok(BeginInstallTxResult::PendingInstallFound(versions));
            }
            BeginInstallResult::PendingUninstallFound(versions) => {
                return Ok(BeginInstallTxResult::PendingUninstallFound(versions));
            }
        }
    }

    Ok(BeginInstallTxResult::CanInstall(DbGuard {
        installation_persisted: false,
        cx: cx.clone(),
        db,
        pkg_id,
    }))
}

#[derive(Debug)]
pub(in crate::command::install) struct DbGuard<'db, S>
where
    S: ReportScope,
{
    installation_persisted: bool,
    cx: ReportContext<BeginInstallScope<S>>,
    db: Arc<Mutex<PackageDatabase<'db>>>,
    pkg_id: PackageId,
}

impl<S> DbGuard<'_, S>
where
    S: ReportScope,
{
    pub(in crate::command::install) fn complete_install(mut self) -> Result<(), S::Error> {
        let mut db = self.db.lock().unwrap();
        db.complete_install(&self.pkg_id)
            .context(CompleteInstallSnafu {
                pkg_id: &self.pkg_id,
            })
            .report_error(self.cx.reporter())?;
        self.installation_persisted = true;
        Ok(())
    }
}

impl<S> Drop for DbGuard<'_, S>
where
    S: ReportScope,
{
    fn drop(&mut self) {
        if self.installation_persisted {
            return;
        }

        self.cx
            .reporter()
            .report_info(format_args!("rolling back database changes..."));

        let mut db = self.db.lock().unwrap();
        let _ = db
            .cancel_install(&self.pkg_id)
            .context(CancelInstallSnafu {
                pkg_id: &self.pkg_id,
            })
            .report_error(self.cx.reporter());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cli::reporter::RootReportScope as _,
        command::common,
        package::PackageState,
        util::testing::{self, TempdirContext, TestScope},
    };

    fn get_entry_state(db: &PackageDatabase<'_>, pkg_id: &PackageId) -> Option<PackageState> {
        db.entry_by_id(pkg_id).map(|(state, _manifest)| state)
    }

    #[test]
    fn begin_install_persists_pending_install_before_completion() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let mut db_lock_file = common::steps::open_db_lock_file(&cx).unwrap();
        let db = common::steps::load_database(&cx, &mut db_lock_file).unwrap();
        let db = Arc::new(Mutex::new(db));
        let manifest = testing::make_manifest("example-namespace", "example-font", "0.1.0");
        let pkg_id = manifest.metadata.id();

        let guard = match begin_install(&cx, Arc::clone(&db), &manifest).unwrap() {
            BeginInstallTxResult::CanInstall(guard) => guard,
            other => panic!("unexpected begin_install result: {other:?}"),
        };

        {
            let mut db = db.lock().unwrap();
            // Reload from disk while keeping the install guard alive so this assertion verifies
            // `begin_install()` persisted `PendingInstall` before completion, rather than only
            // checking the guard's in-memory DB state.
            db.reload().unwrap();
            assert_eq!(
                get_entry_state(&db, &pkg_id),
                Some(PackageState::PendingInstall)
            );
        }

        drop(guard);
    }

    #[test]
    fn dropping_install_guard_rolls_back_persisted_pending_install() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let mut db_lock_file = common::steps::open_db_lock_file(&cx).unwrap();
        let manifest = testing::make_manifest("example-namespace", "example-font", "0.1.0");
        let pkg_id = manifest.metadata.id();

        {
            let db = common::steps::load_database(&cx, &mut db_lock_file).unwrap();
            let db = Arc::new(Mutex::new(db));
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
        let cx = TestScope::start(&cx);
        let mut db_lock_file = common::steps::open_db_lock_file(&cx).unwrap();
        let manifest = testing::make_manifest("example-namespace", "example-font", "0.1.0");
        let pkg_id = manifest.metadata.id();

        {
            let db = common::steps::load_database(&cx, &mut db_lock_file).unwrap();
            let db = Arc::new(Mutex::new(db));
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
