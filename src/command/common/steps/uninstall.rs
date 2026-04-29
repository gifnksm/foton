use std::sync::Arc;

use crate::{
    cli::context::StepContext,
    db::{BeginUninstallResult, PackageDatabase, PackageDatabaseError},
    package::{self, PackageDirs, PackageId},
    platform::windows::steps::unregistration,
    util::{
        fs::FsError,
        reporter::{NeverReport, ReportValue, Step, StepResultErrorExt as _},
    },
};

#[derive(Debug)]
struct UninstallTxStep<S> {
    step: Arc<S>,
}

impl<S> Step for UninstallTxStep<S>
where
    S: Step,
{
    type WarnReportValue = NeverReport;
    type ErrorReportValue = UninstallTxErrorReport;
    type Error = S::Error;

    fn make_failed(&self) -> Self::Error {
        self.step.make_failed()
    }
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
enum UninstallTxErrorReport {
    #[display("resolved package not found in database: {pkg_id}")]
    ResolvedPackageNotFound { pkg_id: PackageId },
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

impl From<UninstallTxErrorReport> for ReportValue<'static> {
    fn from(report: UninstallTxErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

pub(in crate::command) fn uninstall_transaction<S>(
    cx: &StepContext<S>,
    db: &mut PackageDatabase<'_>,
    pkg_id: &PackageId,
) -> Result<(), S::Error>
where
    S: Step,
{
    let cx = cx.with_step(UninstallTxStep {
        step: Arc::clone(cx.step()),
    });
    let reporter = cx.reporter();
    reporter.report_step(format_args!(
        "Beginning transaction to uninstall {pkg_id}..."
    ));

    match db.begin_uninstall(pkg_id) {
        BeginUninstallResult::CanUninstall => {}
        BeginUninstallResult::NotFound => {
            return Err(reporter.report_error({
                let pkg_id = pkg_id.clone();
                UninstallTxErrorReport::ResolvedPackageNotFound { pkg_id }
            }));
        }
    }
    save(&cx, db)?;

    unregistration::unregister_package_fonts(&cx, pkg_id)?;

    let pkg_dirs = PackageDirs::new(cx.app_dirs(), pkg_id);
    package::remove_package_dirs(&pkg_dirs)
        .map_err(|source| {
            let pkg_id = pkg_id.clone();
            UninstallTxErrorReport::RemovePackageFiles { pkg_id, source }
        })
        .report_error(reporter)?;

    // `begin_uninstall` succeeded and the uninstall side effects completed just above, so
    // finalizing the same uninstall in the package database should not fail. If it does, the
    // package database state is internally inconsistent and we intentionally panic.
    db.complete_uninstall(pkg_id).unwrap();
    save(&cx, db)?;

    Ok(())
}

fn save<S>(
    cx: &StepContext<UninstallTxStep<S>>,
    db: &mut PackageDatabase<'_>,
) -> Result<(), S::Error>
where
    S: Step,
{
    db.save()
        .map_err(|source| UninstallTxErrorReport::SaveDatabase { source })
        .report_error(cx.reporter())?;
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

    use crate::{
        command::common,
        db::{BeginInstallResult, PackageDatabase},
        package::{PackageId, PackageState},
        util::testing::{self, TempdirContext, TestStep},
    };

    use super::*;

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
        let cx = cx.with_step(TestStep {});
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
                db.begin_install(&manifest),
                BeginInstallResult::CanInstall
            ));
            db.complete_install(&PKG_ID).unwrap();
            db.save().unwrap();

            uninstall_transaction(&cx, &mut db, &PKG_ID).unwrap();
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
