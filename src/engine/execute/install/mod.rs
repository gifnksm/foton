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
    db::PackageDatabase,
    engine::{execute::install::db_guard::InstallDbGuard, support},
    package::{self, PackageDirs, PackageId, PackageManifest},
    platform::windows::steps::unregistration,
    util::fs::FsError,
};

mod db_guard;
mod package_dirs_guard;
mod registration;

#[derive(Debug)]
struct InstallExecutionScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for InstallExecutionScope<S>
where
    S: ReportScope,
{
    type NoticeReportValue = NeverReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = InstallExecutionErrorReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for InstallExecutionScope<S>
where
    S: ReportScope,
{
    fn new() -> Self {
        Self {
            _base_scope: PhantomData,
        }
    }
}

impl From<InstallExecutionErrorReport> for ReportValue<'static> {
    fn from(report: InstallExecutionErrorReport) -> Self {
        Self::BoxedError(report.into())
    }
}

#[derive(Debug, Snafu)]
enum InstallExecutionErrorReport {
    #[snafu(display(
        concat!(
            "failed to remove package files for package {pkg_id}\n",
            "manual cleanup may be required",
        ),
        pkg_id = pkg_id,
    ))]
    RemovePackageFiles { pkg_id: PackageId, source: FsError },
}

#[derive(Debug)]
pub(in crate::engine) struct InstallExecution<'db, S>
where
    S: ReportScope,
{
    db_guard: InstallDbGuard<'db, S>,
    manifest: Arc<PackageManifest>,
}

impl<'db, S> InstallExecution<'db, S>
where
    S: ReportScope,
{
    pub(in crate::engine) fn new(
        cx: &ReportContext<S>,
        db: Arc<Mutex<PackageDatabase<'db>>>,
        manifest: Arc<PackageManifest>,
    ) -> Self {
        let db_guard = db_guard::begin_install(cx, db, &manifest);
        Self { db_guard, manifest }
    }

    pub(in crate::engine) async fn execute(self, cx: &ReportContext<S>) -> Result<(), S::Error> {
        let pkg_id = self.manifest.id();
        let cx =
            InstallExecutionScope::start_with_report(cx, format_args!("Installing {pkg_id}..."));

        let pkg_dirs = PackageDirs::new(cx.app_dirs(), &pkg_id);
        unregistration::unregister_package_fonts(&cx, &pkg_id)?;
        package::remove_package_dirs(&pkg_dirs)
            .context(RemovePackageFilesSnafu { pkg_id: &pkg_id })
            .report_error(cx.reporter())?;

        let pkg_dirs_guard = package_dirs_guard::create_new_package_dirs(&cx, &pkg_id)?;
        let (package, _) = support::stage_package(&cx, &pkg_dirs_guard, &self.manifest).await?;

        let registration_guard = registration::register_package_fonts(&cx, &package)?;

        self.db_guard.complete_install()?;

        pkg_dirs_guard.disarm();
        registration_guard.disarm();

        Ok(())
    }
}
