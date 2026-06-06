use std::{
    marker::PhantomData,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use snafu::{IntoError as _, ResultExt as _, Snafu};

use crate::{
    cli::{
        context::ReportContext,
        reporter::{
            NeverReport, ReportScope, ReportValue, ScopeResultErrorExt as _, SubReportScope,
        },
    },
    db::{PackageDatabaseError, PackageDatabaseTransaction},
    engine::{
        InstallOp,
        execute::{
            ExecuteErrorReport, PreparedStepOp, install::package_dirs_guard::PackageDirsGuard,
        },
        support,
    },
    package::{self, Package, PackageDirs, PackageId, PackageManifest},
    platform::windows::steps::unregistration,
    util::{fs::FsError, macros::concat_line},
};

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
pub(in crate::engine) struct PreparedInstallStep<S>
where
    S: ReportScope,
{
    manifest: Arc<PackageManifest>,
    cleanup_tracker: CleanupTracker,
    package: Option<Package>,
    pkg_dirs_guard: Option<PackageDirsGuard<InstallExecutionScope<S>>>,
    registration_guard: Option<registration::RegistrationGuard<InstallExecutionScope<S>>>,
    installation_persisted: bool,
}

impl<S> PreparedInstallStep<S>
where
    S: ReportScope,
{
    pub(in crate::engine) fn from_plan_step(
        tx: &mut PackageDatabaseTransaction<'_, '_>,
        step: InstallOp,
    ) -> Self {
        let InstallOp { manifest, .. } = step;
        tx.begin_install(Arc::clone(&manifest));
        Self {
            manifest,
            cleanup_tracker: CleanupTracker::default(),
            package: None,
            pkg_dirs_guard: None,
            registration_guard: None,
            installation_persisted: false,
        }
    }
}

#[async_trait]
impl<S> PreparedStepOp<S> for PreparedInstallStep<S>
where
    S: ReportScope,
{
    async fn execute(&mut self, cx: &ReportContext<S>) -> Result<(), S::Error> {
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

        self.pkg_dirs_guard = Some(package_dirs_guard::create_new_package_dirs(
            &cx,
            self.cleanup_tracker.clone(),
            &pkg_id,
        )?);
        let pkg_dirs_guard = self.pkg_dirs_guard.as_ref().unwrap();
        let (package, _) = support::stage_package(&cx, pkg_dirs_guard, &self.manifest).await?;

        self.registration_guard = Some(registration::register_package_fonts(
            &cx,
            self.cleanup_tracker.clone(),
            &package,
        )?);
        self.package = Some(package);

        Ok(())
    }

    fn on_complete(
        &mut self,
        cx: &ReportContext<S>,
        tx: &mut PackageDatabaseTransaction<'_, '_>,
    ) -> Result<(), S::Error> {
        let cx = InstallExecutionScope::start(cx);

        let package = self.package.as_ref().unwrap();
        tx.complete_install(&self.manifest.id(), package.entries())
            .context(CompleteInstallSnafu {
                pkg_id: &self.manifest.id(),
            })
            .report_error(cx.reporter())?;

        Ok(())
    }

    fn after_commit(&mut self) {
        if let Some(guard) = self.pkg_dirs_guard.take() {
            guard.disarm();
        }
        if let Some(guard) = self.registration_guard.take() {
            guard.disarm();
        }
        self.installation_persisted = true;
    }

    fn on_failure(&mut self, cx: &ReportContext<S>, tx: &mut PackageDatabaseTransaction<'_, '_>) {
        assert!(!self.installation_persisted);

        let cx = InstallExecutionScope::start(cx);
        cx.reporter()
            .report_info(format_args!("rolling back database changes..."));

        let _ = self.registration_guard.take();
        let _ = self.pkg_dirs_guard.take();

        let cleanup_required = self.cleanup_tracker.cleanup_required();
        let _ = tx
            .cancel_install(&self.manifest.id(), cleanup_required)
            .context(CancelInstallSnafu {
                pkg_id: &self.manifest.id(),
            })
            .report_error(cx.reporter());
    }

    fn commit_error_report(&self, source: PackageDatabaseError) -> ExecuteErrorReport {
        super::CommitInstallTransactionSnafu {
            pkg_id: self.manifest.id(),
        }
        .into_error(source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cli::reporter::RootReportScope as _,
        engine::InstallReason,
        package::InstallationState,
        util::testing::{self, TempdirContext, TestScope},
    };

    fn install_op(manifest: &Arc<PackageManifest>) -> InstallOp {
        InstallOp {
            manifest: Arc::clone(manifest),
            reason: InstallReason::RequestedByUser,
        }
    }

    fn set_staged_package(
        step: &mut PreparedInstallStep<TestScope>,
        cx: &ReportContext<TestScope>,
    ) {
        let pkg_id = step.manifest.id();
        step.package = Some(Package::new(
            pkg_id.clone(),
            PackageDirs::new(cx.app_dirs(), &pkg_id),
            vec![],
        ));
    }

    #[test]
    fn prepared_install_step_from_plan_step_begins_incomplete_install_in_transaction() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let manifest = testing::make_manifest("example-font@0.1.0");
        let pkg_id = manifest.id();

        testing::with_db(&cx, |mut db| {
            let mut tx = db.transaction();
            let step: PreparedInstallStep<TestScope> =
                PreparedInstallStep::from_plan_step(&mut tx, install_op(&manifest));
            tx.commit().unwrap();

            assert_eq!(
                db.entry_by_id(&pkg_id).unwrap().installation_state,
                InstallationState::IncompleteInstall,
            );
            assert_eq!(step.manifest.id(), pkg_id);
        });
    }

    #[test]
    fn prepared_install_step_after_commit_marks_installation_persisted() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let manifest = testing::make_manifest("example-font@0.1.0");
        let pkg_id = manifest.id();

        testing::with_db(&cx, |mut db| {
            let mut tx = db.transaction();
            let mut step = PreparedInstallStep::from_plan_step(&mut tx, install_op(&manifest));
            set_staged_package(&mut step, &cx);

            step.on_complete(&cx, &mut tx).unwrap();
            assert!(!step.installation_persisted);

            tx.commit().unwrap();
            assert_eq!(
                db.entry_by_id(&pkg_id).unwrap().installation_state,
                InstallationState::Installed,
            );
            assert!(!step.installation_persisted);

            step.after_commit();

            assert!(step.installation_persisted);
        });
    }

    #[test]
    fn prepared_install_step_on_failure_removes_incomplete_install_without_cleanup() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let manifest = testing::make_manifest("example-font@0.1.0");
        let pkg_id = manifest.id();

        testing::with_db(&cx, |mut db| {
            let mut tx = db.transaction();
            let mut step = PreparedInstallStep::from_plan_step(&mut tx, install_op(&manifest));

            step.on_failure(&cx, &mut tx);
            tx.commit().unwrap();

            assert!(db.entry_by_id(&pkg_id).is_none());
        });
    }

    #[test]
    fn prepared_install_step_on_failure_marks_incomplete_uninstall_when_cleanup_is_required() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let manifest = testing::make_manifest("example-font@0.1.0");
        let pkg_id = manifest.id();

        testing::with_db(&cx, |mut db| {
            let mut tx = db.transaction();
            let mut step = PreparedInstallStep::from_plan_step(&mut tx, install_op(&manifest));
            step.cleanup_tracker.request_cleanup();

            step.on_failure(&cx, &mut tx);
            tx.commit().unwrap();

            assert_eq!(
                db.entry_by_id(&pkg_id).unwrap().installation_state,
                InstallationState::IncompleteUninstall,
            );
        });
    }
}
