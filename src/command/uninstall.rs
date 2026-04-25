use crate::{
    db::lock::{DbLockFile, DbLockFileError},
    package::{self, Package, PackageId},
    platform::windows::steps::unregistration,
    util::{
        app_dirs::AppDirs,
        fs::FsError,
        reporter::{NeverReport, ReportValue, Reporter, Step, StepResultErrorExt as _},
    },
};

#[derive(Debug)]
struct UninstallStep<'a> {
    pkg_id: &'a PackageId,
}

impl Step for UninstallStep<'_> {
    type WarnReportValue = NeverReport;
    type ErrorReportValue = UninstallErrorReport;
    type Error = UninstallError;

    fn report_prelude(&self, reporter: &Reporter) {
        reporter.report_step(format_args!("Uninstalling {}...", self.pkg_id));
    }

    fn make_failed(&self) -> Self::Error {
        UninstallError::Failed
    }
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum UninstallErrorReport {
    #[display("failed to open database lock file")]
    OpenDbLockFile {
        #[error(source)]
        source: DbLockFileError,
    },
    #[display("another install or uninstall operation is already in progress")]
    DbAlreadyLocked {
        #[error(source)]
        source: DbLockFileError,
    },
    #[display("failed to acquire database lock")]
    AcquireDbLock {
        #[error(source)]
        source: DbLockFileError,
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

impl From<UninstallErrorReport> for ReportValue<'static> {
    fn from(report: UninstallErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum UninstallError {
    #[display("failed to uninstall package")]
    Failed,
}

pub(crate) fn uninstall_package(
    reporter: &Reporter,
    app_id: &str,
    app_dirs: &AppDirs,
    package: &Package,
) -> Result<(), UninstallError> {
    let reporter = reporter.with_step(UninstallStep {
        pkg_id: package.id(),
    });

    let mut db_lock = DbLockFile::open(app_dirs)
        .map_err(|source| UninstallErrorReport::OpenDbLockFile { source })
        .report_error(&reporter)?;
    let _db_lock_guard = db_lock
        .try_acquire()
        .map_err(|source| match source {
            DbLockFileError::AlreadyLocked { .. } => {
                UninstallErrorReport::DbAlreadyLocked { source }
            }
            _ => UninstallErrorReport::AcquireDbLock { source },
        })
        .report_error(&reporter)?;

    unregistration::unregister_package_fonts(&reporter, app_id, package.id())?;

    package::remove_package_dirs(package.dirs())
        .map_err(|source| {
            let pkg_id = package.id().clone();
            UninstallErrorReport::RemovePackageFiles { pkg_id, source }
        })
        .report_error(&reporter)?;

    Ok(())
}
