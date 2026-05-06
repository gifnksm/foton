use std::{
    marker::PhantomData,
    sync::{Arc, Mutex},
};

use snafu::{ResultExt as _, Snafu};

use crate::{
    cli::{
        context::ReportContext,
        reporter::{
            NeverReport, ReportScope, ReportValue, ScopeResultErrorExt as _, SubReportScope,
        },
    },
    db::{PackageDatabase, PackageDatabaseError},
    package::{self, PackageDirs, PackageId, PackageState},
    platform::windows::steps::unregistration,
    util::fs::FsError,
};

#[derive(Debug)]
struct UninstallExecutionScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for UninstallExecutionScope<S>
where
    S: ReportScope,
{
    type WarnReportValue = NeverReport;
    type ErrorReportValue = UninstallExecutionErrorReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for UninstallExecutionScope<S>
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
enum UninstallExecutionErrorReport {
    #[snafu(display(
        concat!(
            "failed to remove package files for package {pkg_id}\n",
            "manual cleanup may be required",
        ),
        pkg_id = pkg_id,
    ))]
    RemovePackageFiles { pkg_id: PackageId, source: FsError },
    #[snafu(display(
        concat!(
            "failed to finalize uninstall transaction for package {pkg_id}\n",
            "font unregistration and package file removal may already have been applied\n",
            "rerunning the uninstall command or manual database cleanup may be required"
        ),
        pkg_id = pkg_id,
    ))]
    CompleteUninstall {
        pkg_id: PackageId,
        source: PackageDatabaseError,
    },
}

impl From<UninstallExecutionErrorReport> for ReportValue<'static> {
    fn from(report: UninstallExecutionErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

#[derive(Debug)]
pub(crate) struct UninstallExecution<'db> {
    db: Arc<Mutex<PackageDatabase<'db>>>,
    pkg_id: PackageId,
}

impl<'db> UninstallExecution<'db> {
    pub(crate) fn new(db: Arc<Mutex<PackageDatabase<'db>>>, pkg_id: PackageId) -> Self {
        Self { db, pkg_id }
    }

    pub(crate) fn execute<S>(self, cx: &ReportContext<S>) -> Result<(), S::Error>
    where
        S: ReportScope,
    {
        execute_uninstall(cx, &self.db, &self.pkg_id)
    }
}

fn execute_uninstall<S>(
    cx: &ReportContext<S>,
    db: &Arc<Mutex<PackageDatabase<'_>>>,
    pkg_id: &PackageId,
) -> Result<(), S::Error>
where
    S: ReportScope,
{
    let cx =
        UninstallExecutionScope::start_with_report(cx, format_args!("Uninstalling {pkg_id}..."));
    let reporter = cx.reporter();

    {
        let db = db.lock().unwrap();
        assert_eq!(
            db.entry_by_id(pkg_id).map(|(state, _)| state),
            Some(PackageState::PendingUninstall)
        );
    }

    unregistration::unregister_package_fonts(&cx, pkg_id)?;

    let pkg_dirs = PackageDirs::new(cx.app_dirs(), pkg_id);
    package::remove_package_dirs(&pkg_dirs)
        .context(RemovePackageFilesSnafu { pkg_id })
        .report_error(reporter)?;

    {
        let mut db = db.lock().unwrap();
        db.complete_uninstall(pkg_id)
            .context(CompleteUninstallSnafu { pkg_id })
            .report_error(reporter)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs, process,
        sync::{
            LazyLock,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use super::*;
    use crate::{
        cli::reporter::RootReportScope as _,
        db::PackageDatabase,
        engine,
        package::{PackageId, PackageState},
        util::testing::{self, TempdirContext, TestScope},
    };

    fn test_app_id() -> String {
        static TEST_ID: AtomicUsize = AtomicUsize::new(0);
        format!(
            "io.github.gifnksm.foton.test.uninstall-transaction.{}.{}",
            process::id(),
            TEST_ID.fetch_add(1, Ordering::Relaxed)
        )
    }

    static PKG_ID: LazyLock<PackageId> =
        LazyLock::new(|| "example-namespace/example-font@0.1.0".parse().unwrap());

    fn get_entry_state(db: &PackageDatabase<'_>, pkg_id: &PackageId) -> Option<PackageState> {
        db.entry_by_id(pkg_id).map(|(state, _manifest)| state)
    }

    #[test]
    #[should_panic(expected = "assertion `left == right` failed")]
    fn execute_uninstall_requires_pending_uninstall_state() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        testing::with_db(&cx, |db| {
            let db = Arc::new(Mutex::new(db));
            let _ = execute_uninstall(&cx, &db, &PKG_ID);
        });
    }

    #[test]
    #[cfg_attr(
        not(build_for_sandbox),
        ignore = "registry should be isolated in sandbox tests. use `cargo xtask sandbox run --test` instead."
    )]
    fn execute_uninstall_removes_db_record_and_package_files_on_success() {
        let cx = TempdirContext::with_app_id(test_app_id());
        let cx = TestScope::start(&cx);
        let pkg_dirs = PackageDirs::new(cx.app_dirs(), &PKG_ID);
        fs::create_dir_all(pkg_dirs.fonts_dir()).unwrap();
        fs::write(pkg_dirs.fonts_dir().join("example.ttf"), b"font").unwrap();

        let mut db_lock_file = engine::open_db_lock_file(&cx).unwrap();
        let manifest = testing::make_manifest(&*PKG_ID);
        {
            let mut db = engine::load_database(&cx, &mut db_lock_file).unwrap();
            testing::mark_as_pending_uninstalled(&mut db, &manifest);
            let db = Arc::new(Mutex::new(db));
            execute_uninstall(&cx, &db, &PKG_ID).unwrap();
        }

        {
            let db = engine::load_database(&cx, &mut db_lock_file).unwrap();
            assert_eq!(get_entry_state(&db, &PKG_ID), None);
            assert!(!pkg_dirs.fonts_dir().exists());
            assert!(!pkg_dirs.version_dir().exists());
            assert!(!pkg_dirs.name_dir().exists());
            assert!(!pkg_dirs.namespace_dir().exists());
        }
    }
}
