use std::marker::PhantomData;

use snafu::{ResultExt as _, Snafu};

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
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for DatabaseLoadScope<S>
where
    S: ReportScope,
{
    type NoticeReportValue = NeverReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = DatabaseLoadErrorReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for DatabaseLoadScope<S>
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
enum DatabaseLoadErrorReport {
    #[snafu(transparent)]
    DbLockFile { source: DbLockFileError },
    #[snafu(display("failed to load package database"))]
    LoadDatabase { source: PackageDatabaseError },
}

impl From<DatabaseLoadErrorReport> for ReportValue<'static> {
    fn from(report: DatabaseLoadErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

pub(crate) fn open_db_lock_file<S>(cx: &ReportContext<S>) -> Result<DbLockFile, S::Error>
where
    S: ReportScope,
{
    let cx = DatabaseLoadScope::start(cx);
    DbLockFile::open(cx.app_dirs())
        .map_err(DatabaseLoadErrorReport::from)
        .report_error(cx.reporter())
}

pub(crate) fn load_database<'a, S>(
    cx: &ReportContext<S>,
    lock_file: &'a mut DbLockFile,
) -> Result<PackageDatabase<'a>, S::Error>
where
    S: ReportScope,
{
    let cx = DatabaseLoadScope::start(cx);

    let lock_file_guard = lock_file
        .try_acquire()
        .map_err(DatabaseLoadErrorReport::from)
        .report_error(cx.reporter())?;
    let db = PackageDatabase::load(cx.app_dirs(), lock_file_guard)
        .context(LoadDatabaseSnafu)
        .report_error(cx.reporter())?;
    Ok(db)
}
