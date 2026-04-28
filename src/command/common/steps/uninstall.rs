use std::sync::Arc;

use crate::{
    db::{BeginUninstallResult, PackageDatabase, PackageDatabaseError},
    package::{self, PackageDirs, PackageId},
    platform::windows::steps::unregistration,
    util::{
        app_dirs::AppDirs,
        fs::FsError,
        reporter::{NeverReport, ReportValue, Step, StepReporter, StepResultErrorExt as _},
    },
};

#[derive(Debug)]
struct UninstallTxStep<S> {
    step: Arc<S>,
    pkg_id: PackageId,
}

impl<S> Step for UninstallTxStep<S>
where
    S: Step,
{
    type WarnReportValue = NeverReport;
    type ErrorReportValue = UninstallTxErrorReport;
    type Error = S::Error;

    fn report_prelude(&self, reporter: &crate::util::reporter::RootReporter) {
        reporter.report_step(format_args!(
            "Beginning transaction to uninstall {}...",
            self.pkg_id
        ));
    }

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
    reporter: &StepReporter<S>,
    app_id: &str,
    db: &mut PackageDatabase<'_>,
    app_dirs: &AppDirs,
    pkg_id: &PackageId,
) -> Result<(), S::Error>
where
    S: Step,
{
    let reporter = reporter.with_step(UninstallTxStep {
        step: Arc::clone(reporter.step()),
        pkg_id: pkg_id.clone(),
    });

    match db.begin_uninstall(pkg_id) {
        BeginUninstallResult::CanUninstall => {}
        BeginUninstallResult::NotFound => {
            return Err(reporter.report_error({
                let pkg_id = pkg_id.clone();
                UninstallTxErrorReport::ResolvedPackageNotFound { pkg_id }
            }));
        }
    }
    save(&reporter, db)?;

    unregistration::unregister_package_fonts(&reporter, app_id, pkg_id)?;

    let pkg_dirs = PackageDirs::new(app_dirs, pkg_id);
    package::remove_package_dirs(&pkg_dirs)
        .map_err(|source| {
            let pkg_id = pkg_id.clone();
            UninstallTxErrorReport::RemovePackageFiles { pkg_id, source }
        })
        .report_error(&reporter)?;

    // `begin_uninstall` succeeded and the uninstall side effects completed just above, so
    // finalizing the same uninstall in the package database should not fail. If it does, the
    // package database state is internally inconsistent and we intentionally panic.
    db.complete_uninstall(pkg_id).unwrap();
    save(&reporter, db)?;

    Ok(())
}

fn save<S>(
    reporter: &StepReporter<UninstallTxStep<S>>,
    db: &mut PackageDatabase<'_>,
) -> Result<(), S::Error>
where
    S: Step,
{
    db.save()
        .map_err(|source| UninstallTxErrorReport::SaveDatabase { source })
        .report_error(reporter)?;
    Ok(())
}
