use std::marker::PhantomData;

use snafu::{ResultExt as _, Snafu};

use crate::{
    cli::{
        context::ReportContext,
        reporter::{NeverReport, ReportScope, ScopeResultErrorExt as _, SubReportScope},
    },
    db::{DbLockFile, DbLockFileError, DbLockFileGuard, PackageDatabase, PackageDatabaseError},
};

#[derive(Debug, Default)]
struct DatabaseLoadScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for DatabaseLoadScope<S>
where
    S: ReportScope,
{
    type NoticeReportValue = DatabaseLoadNoticeReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = DatabaseLoadErrorReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for DatabaseLoadScope<S> where S: ReportScope {}

#[derive(Debug, Snafu)]
enum DatabaseLoadNoticeReport {
    #[snafu(display("database is currently locked by another operation; waiting..."))]
    WaitingForLock,
}

#[derive(Debug, Snafu)]
enum DatabaseLoadErrorReport {
    #[snafu(transparent)]
    DbLockFile { source: DbLockFileError },
    #[snafu(display("failed to load package database"))]
    LoadDatabase { source: PackageDatabaseError },
}

pub(crate) fn open_db_lock_file<S>(cx: &ReportContext<S>) -> Result<DbLockFile, S::Error>
where
    S: ReportScope,
{
    let cx = DatabaseLoadScope::start(cx);
    DbLockFile::open(cx.app_dirs())
        .map_err(DatabaseLoadErrorReport::from)
        .report_error(&cx)
}

pub(crate) fn load_database<'a, S>(
    cx: &ReportContext<S>,
    lock_file: &'a mut DbLockFile,
    exit_on_lock: bool,
) -> Result<PackageDatabase<'a>, S::Error>
where
    S: ReportScope,
{
    let cx = DatabaseLoadScope::start(cx);

    let lock_file_guard = lock_database(&cx, lock_file, exit_on_lock)?;
    let db = PackageDatabase::load(cx.app_dirs(), lock_file_guard)
        .context(LoadDatabaseSnafu)
        .report_error(&cx)?;
    Ok(db)
}

fn lock_database<'a, S>(
    cx: &ReportContext<DatabaseLoadScope<S>>,
    lock_file: &'a mut DbLockFile,
    exit_on_lock: bool,
) -> Result<DbLockFileGuard<'a>, S::Error>
where
    S: ReportScope,
{
    let guard = if exit_on_lock {
        lock_file.try_lock()
    } else {
        lock_file.lock(|| cx.reporter().report_notice(WaitingForLockSnafu.build()))
    }
    .map_err(DatabaseLoadErrorReport::from)
    .report_error(cx)?;
    Ok(guard)
}
