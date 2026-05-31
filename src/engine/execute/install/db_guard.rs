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
    engine::execute::install::CleanupTracker,
    package::{PackageId, PackageManifest, PackageState},
    util::macros::concat_line,
};

#[derive(Debug)]
struct InstallDbGuardScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for InstallDbGuardScope<S>
where
    S: ReportScope,
{
    type NoticeReportValue = NeverReport;
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
        concat_line!(
            "failed to roll back install transaction for package {pkg_id}",
            "run `foton repair {pkg_id}` to retry cleanup",
            "if repair does not resolve the problem, manual database cleanup may be required",
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
    cleanup_tracker: CleanupTracker,
    manifest: &PackageManifest,
    replacing_pkg_ids: Vec<PackageId>,
) -> InstallDbGuard<'db, S>
where
    S: ReportScope,
{
    let cx = InstallDbGuardScope::start(cx);
    let pkg_id = manifest.id();

    {
        let db = db.lock().unwrap();
        assert_eq!(
            db.entry_by_id(&pkg_id).map(|(state, _)| state),
            Some(PackageState::IncompleteInstall)
        );
    }

    InstallDbGuard {
        installation_persisted: false,
        cx: cx.clone(),
        db,
        cleanup_tracker,
        pkg_id,
        replacing_pkg_ids,
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
    cleanup_tracker: CleanupTracker,
    pkg_id: PackageId,
    replacing_pkg_ids: Vec<PackageId>,
}

impl<S> InstallDbGuard<'_, S>
where
    S: ReportScope,
{
    pub(super) fn complete_install(&mut self) -> Result<(), S::Error> {
        let mut db = self.db.lock().unwrap();
        db.complete_install(&self.pkg_id, &self.replacing_pkg_ids)
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
        let cleanup_required = self.cleanup_tracker.cleanup_required();

        let mut db = self.db.lock().unwrap();
        let _ = db
            .cancel_install(&self.pkg_id, cleanup_required)
            .context(CancelInstallSnafu {
                pkg_id: &self.pkg_id,
            })
            .report_error(self.cx.reporter());
    }
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::*;
    use crate::{
        cli::reporter::RootReportScope as _,
        engine,
        package::PackageState,
        util::testing::{self, TempdirContext, TestError, TestScope},
    };

    struct RequestCleanupOnDrop {
        cleanup_tracker: CleanupTracker,
    }

    impl Drop for RequestCleanupOnDrop {
        fn drop(&mut self) {
            self.cleanup_tracker.request_cleanup();
        }
    }

    fn get_entry_state(db: &PackageDatabase<'_>, pkg_id: &PackageId) -> Option<PackageState> {
        db.entry_by_id(pkg_id).map(|(state, _manifest)| state)
    }

    #[test]
    fn install_db_guard_preserves_incomplete_install_before_completion() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let manifest = testing::make_manifest("example-font@0.1.0");
        let pkg_id = manifest.id();
        let replacing_pkg_ids = vec![];

        testing::with_db(&cx, |mut db| {
            let plan = testing::make_install_plan(&manifest, vec![]);
            db.apply_plan_transaction(&plan).unwrap();
            let db = Arc::new(Mutex::new(db));
            let cleanup_tracker = CleanupTracker::default();
            let guard = begin_install(
                &cx,
                Arc::clone(&db),
                cleanup_tracker,
                &manifest,
                replacing_pkg_ids,
            );
            {
                let mut db = db.lock().unwrap();
                // Reload from disk while keeping the install guard alive so this assertion verifies
                // `begin_install()` persisted `IncompleteInstall` before completion, rather than only
                // checking the guard's in-memory DB state.
                db.reload().unwrap();
                assert_eq!(
                    get_entry_state(&db, &pkg_id),
                    Some(PackageState::IncompleteInstall)
                );
            }
            drop(guard);
        });
    }

    #[test]
    fn dropping_install_guard_rolls_back_persisted_incomplete_install() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let manifest = testing::make_manifest("example-font@0.1.0");
        let pkg_id = manifest.id();
        let replacing_pkg_ids = vec![];

        let mut db_lock_file = engine::open_db_lock_file(&cx).unwrap();

        {
            let mut db = engine::load_database(&cx, &mut db_lock_file).unwrap();
            let plan = testing::make_install_plan(&manifest, vec![]);
            db.apply_plan_transaction(&plan).unwrap();
            let db = Arc::new(Mutex::new(db));
            let cleanup_tracker = CleanupTracker::default();
            let guard = begin_install(&cx, db, cleanup_tracker, &manifest, replacing_pkg_ids);
            drop(guard);
        }

        {
            let db = engine::load_database(&cx, &mut db_lock_file).unwrap();
            assert_eq!(get_entry_state(&db, &pkg_id), None);
        }
    }

    #[test]
    fn dropping_install_guard_keeps_incomplete_uninstall_when_cleanup_failed() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let manifest = testing::make_manifest("example-font@0.1.0");
        let pkg_id = manifest.id();
        let replacing_pkg_ids = vec![];

        let mut db_lock_file = engine::open_db_lock_file(&cx).unwrap();

        {
            let mut db = engine::load_database(&cx, &mut db_lock_file).unwrap();
            let plan = testing::make_install_plan(&manifest, vec![]);
            db.apply_plan_transaction(&plan).unwrap();
            let db = Arc::new(Mutex::new(db));
            let cleanup_tracker = CleanupTracker::default();
            cleanup_tracker
                .do_base_cleanup(|| Err::<(), ()>(()))
                .unwrap_err();
            let guard = begin_install(&cx, db, cleanup_tracker, &manifest, replacing_pkg_ids);
            drop(guard);
        }

        {
            let db = engine::load_database(&cx, &mut db_lock_file).unwrap();
            assert_eq!(
                get_entry_state(&db, &pkg_id),
                Some(PackageState::IncompleteUninstall)
            );
        }
    }

    #[test]
    fn complete_install_failure_observes_later_cleanup_requests_before_db_rollback() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let manifest = testing::make_manifest("example-font@0.1.0");
        let pkg_id = manifest.id();
        let replacing_pkg_ids = vec!["missing-font@9.9.9".parse().unwrap()];

        let mut db_lock_file = engine::open_db_lock_file(&cx).unwrap();

        {
            let mut db = engine::load_database(&cx, &mut db_lock_file).unwrap();
            let plan = testing::make_install_plan(&manifest, vec![]);
            db.apply_plan_transaction(&plan).unwrap();
            let db = Arc::new(Mutex::new(db));
            let cleanup_tracker = CleanupTracker::default();
            let mut guard = begin_install(
                &cx,
                db,
                cleanup_tracker.clone(),
                &manifest,
                replacing_pkg_ids,
            );
            let _request_cleanup = RequestCleanupOnDrop { cleanup_tracker };

            let err = guard.complete_install().unwrap_err();
            assert_matches!(err, TestError::Failed);
        }

        {
            let db = engine::load_database(&cx, &mut db_lock_file).unwrap();
            assert_eq!(
                get_entry_state(&db, &pkg_id),
                Some(PackageState::IncompleteUninstall)
            );
        }
    }

    #[test]
    fn complete_install_persists_installed_state() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let manifest = testing::make_manifest("example-font@0.1.0");
        let pkg_id = manifest.id();
        let replacing_pkg_ids = vec![];

        let mut db_lock_file = engine::open_db_lock_file(&cx).unwrap();

        {
            let mut db = engine::load_database(&cx, &mut db_lock_file).unwrap();
            let plan = testing::make_install_plan(&manifest, vec![]);
            db.apply_plan_transaction(&plan).unwrap();
            let db = Arc::new(Mutex::new(db));
            let cleanup_tracker = CleanupTracker::default();
            let mut guard = begin_install(&cx, db, cleanup_tracker, &manifest, replacing_pkg_ids);
            guard.complete_install().unwrap();
        }

        {
            let db = engine::load_database(&cx, &mut db_lock_file).unwrap();
            assert_eq!(get_entry_state(&db, &pkg_id), Some(PackageState::Installed));
        }
    }
}
