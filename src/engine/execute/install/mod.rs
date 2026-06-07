use std::{marker::PhantomData, sync::Arc};

use async_trait::async_trait;
use snafu::{IntoError as _, ResultExt as _, Snafu};

use crate::{
    cli::{
        context::ReportContext,
        reporter::{
            NeverReport, ReportScope, ReportValue, ScopeResultErrorExt as _, SubReportScope,
        },
    },
    db::{PackageDatabase, PackageDatabaseError, PackageDatabaseTransaction},
    engine::{
        InstallOp,
        execute::{
            ExecuteErrorReport, PreparedStepOp, install::package_dirs_guard::PackageDirsGuard,
            support::CleanupTracker,
        },
        support,
    },
    package::{self, FontEntry, PackageDirs, PackageId, PackageManifest},
    platform::windows::steps::unregistration::{self, UnregistrationIntent},
    util::{fs::FsError, macros::concat_line},
};

mod package_dirs_guard;

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

#[derive(Debug)]
pub(in crate::engine) struct PreparedInstallStep<S>
where
    S: ReportScope,
{
    manifest: Arc<PackageManifest>,
    cleanup_tracker: CleanupTracker,
    font_entries: Option<Vec<FontEntry>>,
    pkg_dirs_guard: Option<PackageDirsGuard<InstallExecutionScope<S>>>,
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
            font_entries: None,
            pkg_dirs_guard: None,
            installation_persisted: false,
        }
    }
}

#[async_trait]
impl<S> PreparedStepOp<S> for PreparedInstallStep<S>
where
    S: ReportScope,
{
    async fn execute(
        &mut self,
        cx: &ReportContext<S>,
        _db: &PackageDatabase<'_>,
    ) -> Result<(), S::Error> {
        let pkg_id = self.manifest.id();
        let cx =
            InstallExecutionScope::start_with_report(cx, format_args!("Installing {pkg_id}..."));
        let pkg_dirs = PackageDirs::new(cx.app_dirs(), &pkg_id);

        self.cleanup_tracker.do_base_cleanup(|| {
            unregistration::unregister_package_fonts(
                &cx,
                &pkg_id,
                UnregistrationIntent::CleanupBeforeInstall,
            )?;
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
        let (font_entries, _) = support::stage_package(&cx, pkg_dirs_guard, &self.manifest).await?;

        self.font_entries = Some(font_entries);

        Ok(())
    }

    fn on_complete(
        &mut self,
        _cx: &ReportContext<S>,
        tx: &mut PackageDatabaseTransaction<'_, '_>,
    ) -> Result<(), S::Error> {
        let pkg_id = self.manifest.id();
        let font_entries = self.font_entries.as_ref().unwrap();
        tx.complete_install(&pkg_id, font_entries).unwrap();
        Ok(())
    }

    fn after_commit(&mut self) {
        if let Some(guard) = self.pkg_dirs_guard.take() {
            guard.disarm();
        }
        self.installation_persisted = true;
    }

    fn on_failure(&mut self, cx: &ReportContext<S>, tx: &mut PackageDatabaseTransaction<'_, '_>) {
        assert!(!self.installation_persisted);

        let cx = InstallExecutionScope::start(cx);
        cx.reporter()
            .report_info(format_args!("rolling back install changes..."));

        let _ = self.pkg_dirs_guard.take();

        let cleanup_required = self.cleanup_tracker.cleanup_required();
        tx.cancel_install(&self.manifest.id(), cleanup_required)
            .unwrap();
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
    use std::sync::LazyLock;

    use super::*;
    use crate::{
        engine::InstallReason,
        package::{ActivationState, InstallationState},
        util::testing::{self, TestScope},
    };

    static MANIFEST: LazyLock<Arc<PackageManifest>> =
        LazyLock::new(|| testing::make_manifest("example-font@0.1.0"));
    static PKG_ID: LazyLock<PackageId> = LazyLock::new(|| MANIFEST.id());

    fn install_op(manifest: &Arc<PackageManifest>) -> InstallOp {
        InstallOp {
            manifest: Arc::clone(manifest),
            reason: InstallReason::RequestedByUser,
        }
    }

    #[test]
    fn prepared_install_step_from_plan_step_begins_incomplete_install_in_transaction() {
        testing::with_db(|_cx, db| {
            let mut tx = db.transaction();
            let step: PreparedInstallStep<TestScope> =
                PreparedInstallStep::from_plan_step(&mut tx, install_op(&MANIFEST));
            tx.commit().unwrap();

            assert_eq!(
                db.entry_by_id(&PKG_ID).unwrap().installation_state(),
                InstallationState::IncompleteInstall,
            );
            assert_eq!(step.manifest.id(), *PKG_ID);
        });
    }

    #[test]
    fn prepared_install_step_after_commit_persists_installed_inactive_state() {
        testing::with_db(|cx, db| {
            let mut tx = db.transaction();
            let mut step = PreparedInstallStep::from_plan_step(&mut tx, install_op(&MANIFEST));
            step.font_entries = Some(vec![]);

            step.on_complete(cx, &mut tx).unwrap();
            assert!(!step.installation_persisted);

            tx.commit().unwrap();
            let entry = db.entry_by_id(&PKG_ID).unwrap();
            assert_eq!(entry.installation_state(), InstallationState::Installed);
            assert_eq!(entry.activation_state(), ActivationState::Inactive);
            assert!(!step.installation_persisted);

            step.after_commit();

            assert!(step.installation_persisted);
        });
    }

    #[test]
    fn prepared_install_step_on_failure_removes_incomplete_install_without_cleanup() {
        testing::with_db(|cx, db| {
            let mut tx = db.transaction();
            let mut step = PreparedInstallStep::from_plan_step(&mut tx, install_op(&MANIFEST));

            step.on_failure(cx, &mut tx);
            tx.commit().unwrap();

            assert!(db.entry_by_id(&PKG_ID).is_none());
        });
    }

    #[test]
    fn prepared_install_step_on_failure_marks_incomplete_uninstall_when_cleanup_is_required() {
        testing::with_db(|cx, db| {
            let mut tx = db.transaction();
            let mut step = PreparedInstallStep::from_plan_step(&mut tx, install_op(&MANIFEST));
            step.cleanup_tracker.request_cleanup();

            step.on_failure(cx, &mut tx);
            tx.commit().unwrap();

            assert_eq!(
                db.entry_by_id(&PKG_ID).unwrap().installation_state(),
                InstallationState::IncompleteUninstall,
            );
        });
    }
}
