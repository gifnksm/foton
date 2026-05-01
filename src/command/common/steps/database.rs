use std::sync::Arc;

use snafu::{IntoError as _, ResultExt as _, Snafu};

use crate::{
    cli::{
        context::ReportContext,
        reporter::{
            NeverReport, ReportScope, ReportValue, ScopeResultErrorExt as _, SubReportScope,
        },
    },
    db::{DbLockFile, DbLockFileError, PackageDatabase, PackageDatabaseError},
};

#[derive(Debug)]
struct DatabaseLoadScope<S> {
    base_scope: Arc<S>,
}

impl<S> ReportScope for DatabaseLoadScope<S>
where
    S: ReportScope,
{
    type WarnReportValue = NeverReport;
    type ErrorReportValue = DatabaseLoadErrorReport;
    type Error = S::Error;

    fn make_failed(&self) -> Self::Error {
        self.base_scope.make_failed()
    }
}

impl<S> SubReportScope<S> for DatabaseLoadScope<S>
where
    S: ReportScope,
{
    fn new(base_scope: Arc<S>) -> Self {
        Self { base_scope }
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

pub(in crate::command) fn open_db_lock_file<S>(
    cx: &ReportContext<S>,
) -> Result<DbLockFile, S::Error>
where
    S: ReportScope,
{
    let cx = DatabaseLoadScope::start(cx);

    DbLockFile::open(cx.app_dirs())
        .context(OpenDbLockFileSnafu)
        .report_error(cx.reporter())
}

pub(in crate::command) fn load_database<'a, S>(
    cx: &ReportContext<S>,
    lock_file: &'a mut DbLockFile,
) -> Result<PackageDatabase<'a>, S::Error>
where
    S: ReportScope,
{
    let cx = DatabaseLoadScope::start(cx);

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
