use std::marker::PhantomData;

use snafu::{ResultExt as _, Snafu};

use crate::{
    cli::{
        context::ReportContext,
        reporter::{
            NeverReport, ReportScope, ReportValue, ScopeResultErrorExt as _, SubReportScope,
        },
    },
    db::{PackageDatabaseError, PackageDatabaseTransaction},
    engine::UninstallOp,
    package::{self, PackageDirs, PackageId},
    platform::windows::steps::unregistration,
    util::{fs::FsError, macros::concat_line},
};

#[derive(Debug)]
struct UninstallExecutionScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for UninstallExecutionScope<S>
where
    S: ReportScope,
{
    type NoticeReportValue = NeverReport;
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
    #[snafu(display("failed to start uninstall transaction for package {pkg_id}"))]
    BeginUninstall {
        pkg_id: PackageId,
        source: PackageDatabaseError,
    },
    #[snafu(display(
        concat_line!(
            "failed to remove package files for package {pkg_id}",
            "run `foton repair {pkg_id}` to retry cleanup",
            "if repair does not resolve the problem, manual cleanup may be required",
        ),
        pkg_id = pkg_id,
    ))]
    RemovePackageFiles { pkg_id: PackageId, source: FsError },
    #[snafu(display(
        concat_line!(
            "failed to finalize uninstall transaction for package {pkg_id}",
            "font unregistration and package file removal may already have been applied",
            "run `foton repair {pkg_id}` to restore a consistent state",
            "if repair does not resolve the problem, manual database cleanup may be required",
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
pub(in crate::engine) struct PreparedUninstallStep {
    pkg_id: PackageId,
}

impl PreparedUninstallStep {
    pub(in crate::engine) fn from_plan_step<S>(
        cx: &ReportContext<S>,
        tx: &mut PackageDatabaseTransaction<'_, '_>,
        step: UninstallOp,
    ) -> Result<Self, S::Error>
    where
        S: ReportScope,
    {
        let cx = UninstallExecutionScope::start(cx);
        let UninstallOp { pkg_id, .. } = step;

        tx.begin_uninstall(&pkg_id)
            .context(BeginUninstallSnafu { pkg_id: &pkg_id })
            .report_error(cx.reporter())?;
        Ok(Self { pkg_id })
    }

    pub(in crate::engine) fn execute<S>(&mut self, cx: &ReportContext<S>) -> Result<(), S::Error>
    where
        S: ReportScope,
    {
        execute_uninstall(cx, &self.pkg_id)
    }

    pub(in crate::engine) fn on_complete<S>(
        &mut self,
        cx: &ReportContext<S>,
        tx: &mut PackageDatabaseTransaction<'_, '_>,
    ) -> Result<(), S::Error>
    where
        S: ReportScope,
    {
        let cx = UninstallExecutionScope::start(cx);

        tx.complete_uninstall(&self.pkg_id)
            .context(CompleteUninstallSnafu {
                pkg_id: &self.pkg_id,
            })
            .report_error(cx.reporter())?;
        Ok(())
    }

    #[expect(clippy::unused_self)]
    pub(in crate::engine) fn after_commit(&mut self) {}

    #[expect(clippy::unused_self)]
    pub(in crate::engine) fn on_failure<S>(
        &mut self,
        _cx: &ReportContext<S>,
        _tx: &mut PackageDatabaseTransaction<'_, '_>,
    ) where
        S: ReportScope,
    {
        // No rollback is performed on uninstall failure since uninstall steps are designed to be
        // applied incrementally and partially-applied states are expected to be handled by the
        // repair command.
    }
}

fn execute_uninstall<S>(cx: &ReportContext<S>, pkg_id: &PackageId) -> Result<(), S::Error>
where
    S: ReportScope,
{
    let cx =
        UninstallExecutionScope::start_with_report(cx, format_args!("Uninstalling {pkg_id}..."));
    let reporter = cx.reporter();

    unregistration::unregister_package_fonts(&cx, pkg_id)?;

    let pkg_dirs = PackageDirs::new(cx.app_dirs(), pkg_id);
    package::remove_package_dirs(&pkg_dirs)
        .context(RemovePackageFilesSnafu { pkg_id })
        .report_error(reporter)?;

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
        engine::UninstallReason,
        package::{InstallationState, PackageId},
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

    static PKG_ID: LazyLock<PackageId> = LazyLock::new(|| "example-font@0.1.0".parse().unwrap());

    #[test]
    fn prepared_uninstall_step_begins_incomplete_uninstall_in_transaction() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let manifest = testing::make_manifest(&*PKG_ID);

        testing::with_db(&cx, |mut db| {
            testing::mark_as_installed(&mut db, &manifest);
            let step = UninstallOp {
                pkg_id: PKG_ID.clone(),
                reason: UninstallReason::RequestedByUser,
            };

            let mut tx = db.transaction();
            let prepared = PreparedUninstallStep::from_plan_step(&cx, &mut tx, step).unwrap();
            tx.commit().unwrap();

            assert_eq!(
                db.entry_by_id(&PKG_ID).unwrap().installation_state,
                InstallationState::IncompleteUninstall,
            );
            assert_eq!(prepared.pkg_id, *PKG_ID);
        });
    }

    #[test]
    fn prepared_uninstall_step_on_complete_removes_db_record() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let manifest = testing::make_manifest(&*PKG_ID);

        testing::with_db(&cx, |mut db| {
            testing::mark_as_incomplete_uninstall(&mut db, &manifest);
            let mut prepared = PreparedUninstallStep {
                pkg_id: PKG_ID.clone(),
            };

            let mut tx = db.transaction();
            prepared.on_complete(&cx, &mut tx).unwrap();
            tx.commit().unwrap();

            assert!(db.entry_by_id(&PKG_ID).is_none());
        });
    }

    #[test]
    #[cfg_attr(
        not(build_for_sandbox),
        ignore = "registry should be isolated in sandbox tests. use `cargo xtask sandbox run --test` instead."
    )]
    fn execute_uninstall_removes_package_files_on_success() {
        let cx = TempdirContext::with_app_id(test_app_id());
        let cx = TestScope::start(&cx);
        let pkg_dirs = PackageDirs::new(cx.app_dirs(), &PKG_ID);
        fs::create_dir_all(pkg_dirs.fonts_dir()).unwrap();
        fs::write(pkg_dirs.fonts_dir().join("example.ttf"), b"font").unwrap();

        execute_uninstall(&cx, &PKG_ID).unwrap();

        assert!(!pkg_dirs.fonts_dir().exists());
        assert!(!pkg_dirs.version_dir().exists());
        assert!(!pkg_dirs.name_dir().exists());
    }

    #[test]
    #[cfg_attr(
        not(build_for_sandbox),
        ignore = "registry should be isolated in sandbox tests. use `cargo xtask sandbox run --test` instead."
    )]
    fn execute_uninstall_keeps_version_directory_when_leftovers_remain() {
        let cx = TempdirContext::with_app_id(test_app_id());
        let cx = TestScope::start(&cx);
        let pkg_dirs = PackageDirs::new(cx.app_dirs(), &PKG_ID);
        fs::create_dir_all(pkg_dirs.fonts_dir()).unwrap();
        fs::write(pkg_dirs.fonts_dir().join("example.ttf"), b"font").unwrap();
        let leftover = pkg_dirs.version_dir().join("leftover.txt");
        fs::write(&leftover, b"leftover").unwrap();

        execute_uninstall(&cx, &PKG_ID).unwrap_err();

        assert!(!pkg_dirs.fonts_dir().exists());
        assert!(pkg_dirs.version_dir().exists());
        assert!(pkg_dirs.name_dir().exists());
        assert!(leftover.exists());
    }
}
