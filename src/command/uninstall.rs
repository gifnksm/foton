use crate::{
    db::{
        BeginUninstallResult, DbLockFile, DbLockFileError, PackageDatabase, PackageDatabaseError,
    },
    package::{self, PackageDirs, PackageId},
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
    #[display("failed to load package database")]
    LoadDatabase {
        #[error(source)]
        source: PackageDatabaseError,
    },
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
    pkg_id: &PackageId,
) -> Result<(), UninstallError> {
    let reporter = reporter.with_step(UninstallStep { pkg_id });

    let mut db_lock = DbLockFile::open(app_dirs)
        .map_err(|source| UninstallErrorReport::OpenDbLockFile { source })
        .report_error(&reporter)?;
    let db_lock_guard = db_lock
        .try_acquire()
        .map_err(|source| match source {
            DbLockFileError::AlreadyLocked { .. } => {
                UninstallErrorReport::DbAlreadyLocked { source }
            }
            _ => UninstallErrorReport::AcquireDbLock { source },
        })
        .report_error(&reporter)?;

    let mut db = PackageDatabase::load(app_dirs, &db_lock_guard)
        .map_err(|source| UninstallErrorReport::LoadDatabase { source })
        .report_error(&reporter)?;

    match db.begin_uninstall(pkg_id) {
        BeginUninstallResult::CanUninstall => {}
        BeginUninstallResult::NotFound => {
            reporter.report_info(format_args!(
                "package {pkg_id} is not installed; nothing to do"
            ));
            return Ok(());
        }
    }
    db.save()
        .map_err(|source| UninstallErrorReport::SaveDatabase { source })
        .report_error(&reporter)?;

    unregistration::unregister_package_fonts(&reporter, app_id, pkg_id)?;

    let pkg_dirs = PackageDirs::new(app_dirs, pkg_id);
    package::remove_package_dirs(&pkg_dirs)
        .map_err(|source| {
            let pkg_id = pkg_id.clone();
            UninstallErrorReport::RemovePackageFiles { pkg_id, source }
        })
        .report_error(&reporter)?;

    // `begin_uninstall` succeeded and the uninstall side effects completed just above, so
    // finalizing the same uninstall in the package database should not fail. If it does, the
    // package database state is internally inconsistent and we intentionally panic.
    db.complete_uninstall(pkg_id).unwrap();
    db.save()
        .map_err(|source| UninstallErrorReport::SaveDatabase { source })
        .report_error(&reporter)?;

    Ok(())
}
