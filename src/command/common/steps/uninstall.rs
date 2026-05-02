use std::sync::{Arc, Mutex};

use snafu::{ResultExt as _, Snafu};

use crate::{
    cli::{
        context::ReportContext,
        reporter::{
            NeverReport, ReportScope, ReportValue, ScopeResultErrorExt as _, SubReportScope,
        },
    },
    db::{BeginUninstallResult, PackageDatabase, PackageDatabaseError},
    package::{self, PackageDirs, PackageId},
    platform::windows::steps::unregistration,
    util::fs::FsError,
};

#[derive(Debug)]
struct UninstallTxScope<S> {
    base_scope: Arc<S>,
}

impl<S> ReportScope for UninstallTxScope<S>
where
    S: ReportScope,
{
    type WarnReportValue = NeverReport;
    type ErrorReportValue = UninstallTxErrorReport;
    type Error = S::Error;

    fn make_failed(&self) -> Self::Error {
        self.base_scope.make_failed()
    }
}

impl<S> SubReportScope<S> for UninstallTxScope<S>
where
    S: ReportScope,
{
    fn new(base_scope: Arc<S>) -> Self {
        Self { base_scope }
    }
}

#[derive(Debug, Snafu)]
enum UninstallTxErrorReport {
    #[snafu(display("resolved package not found in database: {pkg_id}"))]
    ResolvedPackageNotFound { pkg_id: PackageId },
    #[snafu(display(
        "failed to remove package files for package {pkg_id}\nmanual cleanup may be required"
    ))]
    RemovePackageFiles { pkg_id: PackageId, source: FsError },
    #[snafu(display("failed to begin uninstall transaction for package {pkg_id}"))]
    BeginUninstall {
        pkg_id: PackageId,
        source: PackageDatabaseError,
    },
    #[snafu(display(
        "\
        failed to finalize uninstall transaction for package {pkg_id}\n\
        font unregistration and package file removal may already have been applied\n\
        rerunning the uninstall command or manual database cleanup may be required\
        "
    ))]
    CompleteUninstall {
        pkg_id: PackageId,
        source: PackageDatabaseError,
    },
}

impl From<UninstallTxErrorReport> for ReportValue<'static> {
    fn from(report: UninstallTxErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

pub(in crate::command) fn uninstall_transaction<S>(
    cx: &ReportContext<S>,
    db: &Arc<Mutex<PackageDatabase<'_>>>,
    pkg_id: &PackageId,
) -> Result<(), S::Error>
where
    S: ReportScope,
{
    let cx = UninstallTxScope::start_with_report(
        cx,
        format_args!("Beginning transaction to uninstall {pkg_id}..."),
    );
    let reporter = cx.reporter();

    {
        let mut db = db.lock().unwrap();
        let res = db
            .begin_uninstall(pkg_id)
            .context(BeginUninstallSnafu { pkg_id })
            .report_error(reporter)?;
        match res {
            BeginUninstallResult::CanUninstall => {}
            BeginUninstallResult::NotFound => {
                return Err(reporter.report_error(ResolvedPackageNotFoundSnafu { pkg_id }.build()));
            }
        }
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
        command::common,
        db::{BeginInstallResult, PackageDatabase},
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
    #[cfg_attr(
        not(build_for_sandbox),
        ignore = "registry should be isolated in sandbox tests. use `cargo xtask sandbox run --test` instead."
    )]
    fn uninstall_transaction_removes_db_record_and_package_files_on_success() {
        let cx = TempdirContext::with_app_id(test_app_id());
        let cx = TestScope::start(&cx);
        let pkg_dirs = PackageDirs::new(cx.app_dirs(), &PKG_ID);
        fs::create_dir_all(pkg_dirs.fonts_dir()).unwrap();
        fs::write(pkg_dirs.fonts_dir().join("example.ttf"), b"font").unwrap();

        let mut db_lock_file = common::steps::open_db_lock_file(&cx).unwrap();
        let manifest = testing::make_manifest(
            PKG_ID.namespace().clone(),
            PKG_ID.name().clone(),
            PKG_ID.version().clone(),
        );
        {
            let mut db = common::steps::load_database(&cx, &mut db_lock_file).unwrap();
            assert!(matches!(
                db.begin_install(&manifest).unwrap(),
                BeginInstallResult::CanInstall
            ));
            db.complete_install(&PKG_ID).unwrap();

            let db = Arc::new(Mutex::new(db));
            uninstall_transaction(&cx, &db, &PKG_ID).unwrap();
        }

        {
            let db = common::steps::load_database(&cx, &mut db_lock_file).unwrap();
            assert_eq!(get_entry_state(&db, &PKG_ID), None);
            assert!(!pkg_dirs.fonts_dir().exists());
            assert!(!pkg_dirs.version_dir().exists());
            assert!(!pkg_dirs.name_dir().exists());
            assert!(!pkg_dirs.namespace_dir().exists());
        }
    }
}
