use std::marker::PhantomData;

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
        UninstallOp,
        execute::{ExecuteErrorReport, PreparedStepOp},
    },
    package::{self, PackageDirs, PackageId},
    platform::windows::steps::unregistration,
    util::{fs::FsError, macros::concat_line},
};

#[derive(Debug)]
struct UninstallExecutionScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for UninstallExecutionScope<S>
where
    S: ReportScope,
{
    type NoticeReportValue = NeverReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = UninstallExecutionErrorReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for UninstallExecutionScope<S>
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
enum UninstallExecutionErrorReport {
    #[snafu(display("failed to start uninstall transaction for package {pkg_id}"))]
    BeginUninstall {
        pkg_id: PackageId,
        source: PackageDatabaseError,
    },
    #[snafu(display(
        concat_line!(
            "failed to remove package files for package {pkg_id}",
            "run `foton repair {pkg_id}` to retry cleanup",
            "if repair does not resolve the problem, manual cleanup may be required",
        ),
        pkg_id = pkg_id,
    ))]
    RemovePackageFiles { pkg_id: PackageId, source: FsError },
    #[snafu(display(
        concat_line!(
            "failed to finalize uninstall transaction for package {pkg_id}",
            "font unregistration and package file removal may already have been applied",
            "run `foton repair {pkg_id}` to restore a consistent state",
            "if repair does not resolve the problem, manual database cleanup may be required",
        ),
        pkg_id = pkg_id,
    ))]
    CompleteUninstall {
        pkg_id: PackageId,
        source: PackageDatabaseError,
    },
}

impl From<UninstallExecutionErrorReport> for ReportValue<'static> {
    fn from(report: UninstallExecutionErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

#[derive(Debug)]
pub(in crate::engine) struct PreparedUninstallStep {
    pkg_id: PackageId,
}

impl PreparedUninstallStep {
    pub(in crate::engine) fn from_plan_step<S>(
        cx: &ReportContext<S>,
        tx: &mut PackageDatabaseTransaction<'_, '_>,
        step: UninstallOp,
    ) -> Result<Self, S::Error>
    where
        S: ReportScope,
    {
        let cx = UninstallExecutionScope::start(cx);
        let UninstallOp { pkg_id, .. } = step;

        // TODO: make deactivation part independent
        tx.begin_deactivate(&pkg_id)
            .context(BeginUninstallSnafu { pkg_id: &pkg_id })
            .report_error(cx.reporter())?;
        tx.complete_deactivate(&pkg_id)
            .context(BeginUninstallSnafu { pkg_id: &pkg_id })
            .report_error(cx.reporter())?;
        tx.begin_uninstall(&pkg_id)
            .context(BeginUninstallSnafu { pkg_id: &pkg_id })
            .report_error(cx.reporter())?;
        Ok(Self { pkg_id })
    }
}

#[async_trait]
impl<S> PreparedStepOp<S> for PreparedUninstallStep
where
    S: ReportScope,
{
    async fn execute(&mut self, cx: &ReportContext<S>) -> Result<(), S::Error> {
        execute_uninstall(cx, &self.pkg_id)
    }

    fn on_complete(
        &mut self,
        cx: &ReportContext<S>,
        tx: &mut PackageDatabaseTransaction<'_, '_>,
    ) -> Result<(), S::Error> {
        let cx = UninstallExecutionScope::start(cx);

        tx.complete_uninstall(&self.pkg_id)
            .context(CompleteUninstallSnafu {
                pkg_id: &self.pkg_id,
            })
            .report_error(cx.reporter())?;
        Ok(())
    }

    fn after_commit(&mut self) {}

    fn on_failure(&mut self, _cx: &ReportContext<S>, _tx: &mut PackageDatabaseTransaction<'_, '_>) {
        // No rollback is performed on uninstall failure since uninstall steps are designed to be
        // applied incrementally and partially-applied states are expected to be handled by the
        // repair command.
    }

    fn commit_error_report(&self, source: PackageDatabaseError) -> ExecuteErrorReport {
        super::CommitUninstallTransactionSnafu {
            pkg_id: &self.pkg_id,
        }
        .into_error(source)
    }
}

fn execute_uninstall<S>(cx: &ReportContext<S>, pkg_id: &PackageId) -> Result<(), S::Error>
where
    S: ReportScope,
{
    let cx =
        UninstallExecutionScope::start_with_report(cx, format_args!("Uninstalling {pkg_id}..."));
    let reporter = cx.reporter();

    unregistration::unregister_package_fonts(&cx, pkg_id)?;

    let pkg_dirs = PackageDirs::new(cx.app_dirs(), pkg_id);
    package::remove_package_dirs(&pkg_dirs)
        .context(RemovePackageFilesSnafu { pkg_id })
        .report_error(reporter)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::{Arc, LazyLock},
    };

    use super::*;
    use crate::{
        engine::UninstallReason,
        package::{ActivationState, InstallationState, PackageId, PackageManifest},
        util::testing,
    };

    static MANIFEST: LazyLock<Arc<PackageManifest>> =
        LazyLock::new(|| testing::make_manifest("example-font@0.1.0"));
    static PKG_ID: LazyLock<PackageId> = LazyLock::new(|| MANIFEST.id());

    #[test]
    fn prepared_uninstall_step_from_plan_step_begins_incomplete_uninstall_in_transaction() {
        testing::with_db(|cx, db| {
            testing::mark_as_active(db, &MANIFEST);

            let step = UninstallOp {
                pkg_id: PKG_ID.clone(),
                reason: UninstallReason::RequestedByUser,
            };

            let mut tx = db.transaction();
            let prepared = PreparedUninstallStep::from_plan_step(cx, &mut tx, step).unwrap();
            tx.commit().unwrap();

            let entry = db.entry_by_id(&PKG_ID).unwrap();
            assert_eq!(
                entry.installation_state,
                InstallationState::IncompleteUninstall
            );
            assert_eq!(entry.activation_state, ActivationState::Inactive);
            assert_eq!(prepared.pkg_id, *PKG_ID);
        });
    }

    #[test]
    fn prepared_uninstall_step_on_complete_removes_db_record() {
        testing::with_db(|cx, db| {
            testing::mark_as_incomplete_uninstall(db, &MANIFEST);
            let mut prepared = PreparedUninstallStep {
                pkg_id: PKG_ID.clone(),
            };

            let mut tx = db.transaction();
            prepared.on_complete(cx, &mut tx).unwrap();
            tx.commit().unwrap();

            assert!(db.entry_by_id(&PKG_ID).is_none());
        });
    }

    #[test]
    #[cfg_attr(
        not(build_for_sandbox),
        ignore = "registry should be isolated in sandbox tests. use `cargo xtask sandbox run --test` instead."
    )]
    fn execute_uninstall_removes_package_files_on_success() {
        testing::with_context(|cx| {
            let pkg_dirs = PackageDirs::new(cx.app_dirs(), &PKG_ID);
            fs::create_dir_all(pkg_dirs.fonts_dir()).unwrap();
            fs::write(pkg_dirs.fonts_dir().join("example.ttf"), b"font").unwrap();

            execute_uninstall(cx, &PKG_ID).unwrap();

            assert!(!pkg_dirs.fonts_dir().exists());
            assert!(!pkg_dirs.version_dir().exists());
            assert!(!pkg_dirs.name_dir().exists());
        });
    }

    #[test]
    #[cfg_attr(
        not(build_for_sandbox),
        ignore = "registry should be isolated in sandbox tests. use `cargo xtask sandbox run --test` instead."
    )]
    fn execute_uninstall_keeps_version_directory_when_leftovers_remain() {
        testing::with_context(|cx| {
            let pkg_dirs = PackageDirs::new(cx.app_dirs(), &PKG_ID);
            fs::create_dir_all(pkg_dirs.fonts_dir()).unwrap();
            fs::write(pkg_dirs.fonts_dir().join("example.ttf"), b"font").unwrap();
            let leftover = pkg_dirs.version_dir().join("leftover.txt");
            fs::write(&leftover, b"leftover").unwrap();

            execute_uninstall(cx, &PKG_ID).unwrap_err();

            assert!(!pkg_dirs.fonts_dir().exists());
            assert!(pkg_dirs.version_dir().exists());
            assert!(pkg_dirs.name_dir().exists());
            assert!(leftover.exists());
        });
    }
}
