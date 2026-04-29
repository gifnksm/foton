use std::sync::Arc;

use crate::{
    cli::context::StepContext,
    db::{DbLockFile, DbLockFileError, PackageDatabase, PackageDatabaseError},
    util::reporter::{NeverReport, ReportValue, Step, StepResultErrorExt as _},
};

#[derive(Debug)]
struct DatabaseLoadStep<S> {
    step: Arc<S>,
}

impl<S> Step for DatabaseLoadStep<S>
where
    S: Step,
{
    type WarnReportValue = NeverReport;
    type ErrorReportValue = DatabaseLoadErrorReport;
    type Error = S::Error;

    fn make_failed(&self) -> Self::Error {
        self.step.make_failed()
    }
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
enum DatabaseLoadErrorReport {
    #[display("failed to open database lock file")]
    OpenDbLockFile {
        #[error(source)]
        source: DbLockFileError,
    },
    #[display("another operation is already in progress")]
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

impl From<DatabaseLoadErrorReport> for ReportValue<'static> {
    fn from(report: DatabaseLoadErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

pub(in crate::command) fn open_db_lock_file<S>(cx: &StepContext<S>) -> Result<DbLockFile, S::Error>
where
    S: Step,
{
    let cx = cx.with_step(DatabaseLoadStep {
        step: Arc::clone(cx.step()),
    });

    DbLockFile::open(cx.app_dirs())
        .map_err(|source| DatabaseLoadErrorReport::OpenDbLockFile { source })
        .report_error(cx.reporter())
}

pub(in crate::command) fn load_database<'a, S>(
    cx: &StepContext<S>,
    lock_file: &'a mut DbLockFile,
) -> Result<PackageDatabase<'a>, S::Error>
where
    S: Step,
{
    let cx = cx.with_step(DatabaseLoadStep {
        step: Arc::clone(cx.step()),
    });

    let lock_file_guard = lock_file
        .try_acquire()
        .map_err(|source| match source {
            DbLockFileError::AlreadyLocked { .. } => {
                DatabaseLoadErrorReport::DbAlreadyLocked { source }
            }
            _ => DatabaseLoadErrorReport::AcquireDbLock { source },
        })
        .report_error(cx.reporter())?;
    let db = PackageDatabase::load(cx.app_dirs(), lock_file_guard)
        .map_err(|source| DatabaseLoadErrorReport::LoadDatabase { source })
        .report_error(cx.reporter())?;
    Ok(db)
}
