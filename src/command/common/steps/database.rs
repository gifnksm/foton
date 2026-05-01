use std::sync::Arc;

use snafu::{IntoError as _, ResultExt as _, Snafu};

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

#[derive(Debug, Snafu)]
enum DatabaseLoadErrorReport {
    #[snafu(display("failed to open database lock file"))]
    OpenDbLockFile { source: DbLockFileError },
    #[snafu(display("another operation is already in progress"))]
    DbAlreadyLocked { source: DbLockFileError },
    #[snafu(display("failed to acquire database lock"))]
    AcquireDbLock { source: DbLockFileError },
    #[snafu(display("failed to load package database"))]
    LoadDatabase { source: PackageDatabaseError },
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
        .context(OpenDbLockFileSnafu)
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
            DbLockFileError::AlreadyLocked { .. } => DbAlreadyLockedSnafu.into_error(source),
            _ => AcquireDbLockSnafu.into_error(source),
        })
        .report_error(cx.reporter())?;
    let db = PackageDatabase::load(cx.app_dirs(), lock_file_guard)
        .context(LoadDatabaseSnafu)
        .report_error(cx.reporter())?;
    Ok(db)
}
