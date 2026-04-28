use std::sync::Arc;

use crate::{
    cli::context::StepContext,
    db::{BeginUninstallResult, PackageDatabase, PackageDatabaseError},
    package::{self, PackageDirs, PackageId},
    platform::windows::steps::unregistration,
    util::{
        fs::FsError,
        reporter::{NeverReport, ReportValue, Step, StepResultErrorExt as _},
    },
};

#[derive(Debug)]
struct UninstallTxStep<S> {
    step: Arc<S>,
}

impl<S> Step for UninstallTxStep<S>
where
    S: Step,
{
    type WarnReportValue = NeverReport;
    type ErrorReportValue = UninstallTxErrorReport;
    type Error = S::Error;

    fn make_failed(&self) -> Self::Error {
        self.step.make_failed()
    }
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
enum UninstallTxErrorReport {
    #[display("resolved package not found in database: {pkg_id}")]
    ResolvedPackageNotFound { pkg_id: PackageId },
    #[display("failed to save package database")]
    SaveDatabase {
        #[error(source)]
        source: PackageDatabaseError,
    },
    #[display(
        "failed to remove package files for package {pkg_id}; manual cleanup may be required"
    )]
    RemovePackageFiles {
        pkg_id: PackageId,
        #[error(source)]
        source: FsError,
    },
}

impl From<UninstallTxErrorReport> for ReportValue<'static> {
    fn from(report: UninstallTxErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

pub(in crate::command) fn uninstall_transaction<S>(
    cx: &StepContext<S>,
    db: &mut PackageDatabase<'_>,
    pkg_id: &PackageId,
) -> Result<(), S::Error>
where
    S: Step,
{
    let cx = cx.with_step(UninstallTxStep {
        step: Arc::clone(cx.step()),
    });
    let reporter = cx.reporter();
    reporter.report_step(format_args!(
        "Beginning transaction to uninstall {pkg_id}..."
    ));

    match db.begin_uninstall(pkg_id) {
        BeginUninstallResult::CanUninstall => {}
        BeginUninstallResult::NotFound => {
            return Err(reporter.report_error({
                let pkg_id = pkg_id.clone();
                UninstallTxErrorReport::ResolvedPackageNotFound { pkg_id }
            }));
        }
    }
    save(&cx, db)?;

    unregistration::unregister_package_fonts(&cx, pkg_id)?;

    let pkg_dirs = PackageDirs::new(cx.app_dirs(), pkg_id);
    package::remove_package_dirs(&pkg_dirs)
        .map_err(|source| {
            let pkg_id = pkg_id.clone();
            UninstallTxErrorReport::RemovePackageFiles { pkg_id, source }
        })
        .report_error(reporter)?;

    // `begin_uninstall` succeeded and the uninstall side effects completed just above, so
    // finalizing the same uninstall in the package database should not fail. If it does, the
    // package database state is internally inconsistent and we intentionally panic.
    db.complete_uninstall(pkg_id).unwrap();
    save(&cx, db)?;

    Ok(())
}

fn save<S>(
    cx: &StepContext<UninstallTxStep<S>>,
    db: &mut PackageDatabase<'_>,
) -> Result<(), S::Error>
where
    S: Step,
{
    db.save()
        .map_err(|source| UninstallTxErrorReport::SaveDatabase { source })
        .report_error(cx.reporter())?;
    Ok(())
}
