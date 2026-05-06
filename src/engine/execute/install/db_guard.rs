use std::{
    marker::PhantomData,
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
    db::{PackageDatabase, PackageDatabaseError},
    package::{PackageId, PackageManifest, PackageState},
};

#[derive(Debug)]
struct InstallDbGuardScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for InstallDbGuardScope<S>
where
    S: ReportScope,
{
    type WarnReportValue = NeverReport;
    type ErrorReportValue = InstallDbGuardErrorReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for InstallDbGuardScope<S>
where
    S: ReportScope,
{
    fn new() -> Self {
        Self {
            _base_scope: PhantomData,
        }
    }
}

#[derive(Debug, Snafu)]
enum InstallDbGuardErrorReport {
    #[snafu(display("failed to complete install transaction for package {pkg_id}"))]
    CompleteInstall {
        pkg_id: PackageId,
        source: PackageDatabaseError,
    },
    #[snafu(display(
        concat!(
            "failed to roll back install transaction for package {pkg_id}\n",
            "manual database cleanup may be required"
        ),
        pkg_id = pkg_id,
    ))]
    CancelInstall {
        pkg_id: PackageId,
        source: PackageDatabaseError,
    },
}

impl From<InstallDbGuardErrorReport> for ReportValue<'static> {
    fn from(report: InstallDbGuardErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

pub(super) fn begin_install<'db, S>(
    cx: &ReportContext<S>,
    db: Arc<Mutex<PackageDatabase<'db>>>,
    manifest: &PackageManifest,
) -> InstallDbGuard<'db, S>
where
    S: ReportScope,
{
    let cx = InstallDbGuardScope::start(cx);
    let pkg_id = manifest.metadata.id();

    {
        let db = db.lock().unwrap();
        assert_eq!(
            db.entry_by_id(&pkg_id).map(|(state, _)| state),
            Some(PackageState::PendingInstall)
        );
    }

    InstallDbGuard {
        installation_persisted: false,
        cx: cx.clone(),
        db,
        pkg_id,
    }
}

#[derive(Debug)]
pub(super) struct InstallDbGuard<'db, S>
where
    S: ReportScope,
{
    installation_persisted: bool,
    cx: ReportContext<InstallDbGuardScope<S>>,
    db: Arc<Mutex<PackageDatabase<'db>>>,
    pkg_id: PackageId,
}

impl<S> InstallDbGuard<'_, S>
where
    S: ReportScope,
{
    pub(super) fn complete_install(mut self) -> Result<(), S::Error> {
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

impl<S> Drop for InstallDbGuard<'_, S>
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
        engine,
        package::PackageState,
        util::testing::{self, TempdirContext, TestScope},
    };

    fn get_entry_state(db: &PackageDatabase<'_>, pkg_id: &PackageId) -> Option<PackageState> {
        db.entry_by_id(pkg_id).map(|(state, _manifest)| state)
    }

    #[test]
    fn install_db_guard_preserves_pending_install_before_completion() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let manifest = testing::make_manifest("example-namespace/example-font@0.1.0");
        let pkg_id = manifest.metadata.id();

        testing::with_db(&cx, |mut db| {
            db.apply_install_transaction(&manifest).unwrap();
            let db = Arc::new(Mutex::new(db));
            let guard = begin_install(&cx, Arc::clone(&db), &manifest);
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
        });
    }

    #[test]
    fn dropping_install_guard_rolls_back_persisted_pending_install() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let manifest = testing::make_manifest("example-namespace/example-font@0.1.0");
        let pkg_id = manifest.metadata.id();

        let mut db_lock_file = engine::open_db_lock_file(&cx).unwrap();

        {
            let mut db = engine::load_database(&cx, &mut db_lock_file).unwrap();
            db.apply_install_transaction(&manifest).unwrap();
            let db = Arc::new(Mutex::new(db));
            let guard = begin_install(&cx, db, &manifest);
            drop(guard);
        }

        {
            let db = engine::load_database(&cx, &mut db_lock_file).unwrap();
            assert_eq!(get_entry_state(&db, &pkg_id), None);
        }
    }

    #[test]
    fn complete_install_persists_installed_state() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let manifest = testing::make_manifest("example-namespace/example-font@0.1.0");
        let pkg_id = manifest.metadata.id();

        let mut db_lock_file = engine::open_db_lock_file(&cx).unwrap();

        {
            let mut db = engine::load_database(&cx, &mut db_lock_file).unwrap();
            db.apply_install_transaction(&manifest).unwrap();
            let db = Arc::new(Mutex::new(db));
            let guard = begin_install(&cx, db, &manifest);
            guard.complete_install().unwrap();
        }

        {
            let db = engine::load_database(&cx, &mut db_lock_file).unwrap();
            assert_eq!(get_entry_state(&db, &pkg_id), Some(PackageState::Installed));
        }
    }
}
