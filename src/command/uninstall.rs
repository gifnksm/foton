use crate::{
    cli::context::RootContext,
    command::common,
    db::{DbLockFile, DbLockFileError, PackageDatabase, PackageDatabaseError},
    package::PackageSpec,
    util::reporter::{NeverReport, ReportValue, Step, StepResultErrorExt as _},
};

#[derive(Debug)]
struct UninstallStep {}

impl Step for UninstallStep {
    type WarnReportValue = NeverReport;
    type ErrorReportValue = UninstallErrorReport;
    type Error = UninstallError;

    fn make_failed(&self) -> Self::Error {
        UninstallError::Failed
    }
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
enum UninstallErrorReport {
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
    cx: &RootContext,
    pkg_spec: &PackageSpec,
) -> Result<(), UninstallError> {
    let cx = cx.with_step(UninstallStep {});
    let reporter = cx.reporter();
    reporter.report_step(format_args!("Uninstalling {pkg_spec}..."));

    let mut db_lock = DbLockFile::open(cx.app_dirs())
        .map_err(|source| UninstallErrorReport::OpenDbLockFile { source })
        .report_error(reporter)?;
    let db_lock_guard = db_lock
        .try_acquire()
        .map_err(|source| match source {
            DbLockFileError::AlreadyLocked { .. } => {
                UninstallErrorReport::DbAlreadyLocked { source }
            }
            _ => UninstallErrorReport::AcquireDbLock { source },
        })
        .report_error(reporter)?;

    let mut db = PackageDatabase::load(cx.app_dirs(), db_lock_guard)
        .map_err(|source| UninstallErrorReport::LoadDatabase { source })
        .report_error(reporter)?;

    let Some((_state, manifest)) = common::steps::resolve_spec_in_db(&cx, &db, pkg_spec)? else {
        reporter.report_info(format_args!(
            "no package matches the specified package `{pkg_spec}`; nothing to do"
        ));
        return Ok(());
    };

    let pkg_id = manifest.metadata.id();
    common::steps::uninstall_transaction(&cx, &mut db, &pkg_id)?;

    Ok(())
}
