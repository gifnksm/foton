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
    db::PackageDatabase,
    engine::{ExecutionId, ExecutionResult, execute::install::db_guard::InstallDbGuard, support},
    package::{self, PackageDirs, PackageId, PackageManifest},
    platform::windows::steps::unregistration,
    util::{fs::FsError, macros::concat_line},
};

mod db_guard;
mod package_dirs_guard;
mod registration;

#[derive(Debug)]
struct InstallExecutionScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for InstallExecutionScope<S>
where
    S: ReportScope,
{
    type NoticeReportValue = NeverReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = InstallExecutionErrorReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for InstallExecutionScope<S>
where
    S: ReportScope,
{
    fn new() -> Self {
        Self {
            _base_scope: PhantomData,
        }
    }
}

impl From<InstallExecutionErrorReport> for ReportValue<'static> {
    fn from(report: InstallExecutionErrorReport) -> Self {
        Self::BoxedError(report.into())
    }
}

#[derive(Debug, Snafu)]
enum InstallExecutionErrorReport {
    #[snafu(display(
        concat_line!(
            "failed to remove files from the installation path for package {pkg_id}",
            "run `foton repair {pkg_id}` to retry removing those files",
            "if repair does not resolve the problem, manual cleanup may be required",
        ),
        pkg_id = pkg_id,
    ))]
    RemovePackageFiles { pkg_id: PackageId, source: FsError },
}

// Tracks whether a failed install can be treated as cleanly rolled back.
// If any step may have left registry or filesystem state behind, the DB entry
// should stay recoverable as `IncompleteUninstall` instead of being removed.
#[derive(Debug, Default, Clone)]
struct CleanupTracker {
    inner: Arc<Mutex<CleanupTrackerInner>>,
}

#[derive(Debug, Default)]
struct CleanupTrackerInner {
    base_cleanup_failed: bool,
    rollback_cleanup_failed: bool,
    cleanup_requested: bool,
}

impl CleanupTracker {
    fn cleanup_required(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.base_cleanup_failed || inner.rollback_cleanup_failed || inner.cleanup_requested
    }

    fn request_cleanup(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.cleanup_requested = true;
    }

    fn do_base_cleanup<F, T, E>(&self, f: F) -> Result<T, E>
    where
        F: FnOnce() -> Result<T, E>,
    {
        let res = f();
        if res.is_err() {
            self.inner.lock().unwrap().base_cleanup_failed = true;
        }
        res
    }

    fn do_rollback_cleanup<F, T, E>(&self, f: F) -> Result<T, E>
    where
        F: FnOnce() -> Result<T, E>,
    {
        let res = f();
        if res.is_err() {
            self.inner.lock().unwrap().rollback_cleanup_failed = true;
        }
        res
    }
}

#[derive(Debug)]
pub(in crate::engine) struct InstallExecution<'db, S>
where
    S: ReportScope,
{
    db_guard: InstallDbGuard<'db, S>,
    exec_id: ExecutionId,
    manifest: Arc<PackageManifest>,
    cleanup_tracker: CleanupTracker,
}

impl<'db, S> InstallExecution<'db, S>
where
    S: ReportScope,
{
    pub(in crate::engine) fn new(
        cx: &ReportContext<S>,
        db: Arc<Mutex<PackageDatabase<'db>>>,
        exec_id: ExecutionId,
        manifest: Arc<PackageManifest>,
        replacing_pkg_ids: Vec<PackageId>,
    ) -> Self {
        let cleanup_tracker = CleanupTracker::default();
        let db_guard = db_guard::begin_install(
            cx,
            db,
            cleanup_tracker.clone(),
            &manifest,
            replacing_pkg_ids,
        );
        Self {
            db_guard,
            exec_id,
            manifest,
            cleanup_tracker,
        }
    }

    pub(in crate::engine) async fn execute(
        mut self,
        cx: &ReportContext<S>,
    ) -> Result<(), S::Error> {
        let pkg_id = self.manifest.id();
        let cx =
            InstallExecutionScope::start_with_report(cx, format_args!("Installing {pkg_id}..."));

        let pkg_dirs = PackageDirs::new(cx.app_dirs(), &pkg_id);
        self.cleanup_tracker.do_base_cleanup(|| {
            unregistration::unregister_package_fonts(&cx, &pkg_id)?;
            package::remove_package_dirs(&pkg_dirs)
                .context(RemovePackageFilesSnafu { pkg_id: &pkg_id })
                .report_error(cx.reporter())?;
            Ok(())
        })?;

        let pkg_dirs_guard = package_dirs_guard::create_new_package_dirs(
            &cx,
            self.cleanup_tracker.clone(),
            &pkg_id,
        )?;
        let (package, _) = support::stage_package(&cx, &pkg_dirs_guard, &self.manifest).await?;

        let registration_guard =
            registration::register_package_fonts(&cx, self.cleanup_tracker.clone(), &package)?;

        self.db_guard.complete_install(package.entries())?;

        pkg_dirs_guard.disarm();
        registration_guard.disarm();

        Ok(())
    }

    pub(in crate::engine) fn exec_id(&self) -> ExecutionId {
        self.exec_id
    }

    #[expect(clippy::unused_self)]
    pub(in crate::engine) fn can_execute(&self) -> bool {
        true
    }

    #[expect(clippy::unused_self)]
    pub(in crate::engine) fn notify_result(
        &mut self,
        _exec_id: ExecutionId,
        _result: ExecutionResult,
    ) {
    }
}
