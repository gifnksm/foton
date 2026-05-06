use std::{
    marker::PhantomData,
    sync::{Arc, Mutex},
};

use snafu::{ResultExt as _, Snafu};

use crate::{
    cli::{
        context::ReportContext,
        reporter::{
            NeverReport, OperationError as _, ReportScope, ReportValue, ScopeResultErrorExt as _,
            SubReportScope,
        },
    },
    db::PackageDatabase,
    engine::execute::install::db_guard::InstallDbGuard,
    package::{self, Package, PackageDirs, PackageId, PackageManifest},
    platform::windows::steps::unregistration,
    util::fs::FsError,
};

mod db_guard;
mod download;
mod extract;
mod package_dirs_guard;
mod registration;
mod validate;

#[derive(Debug)]
struct InstallExecutionScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for InstallExecutionScope<S>
where
    S: ReportScope,
{
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
    #[snafu(display("no valid font files found in package {pkg_id}"))]
    NoValidFonts { pkg_id: PackageId },
}

#[derive(Debug)]
pub(crate) struct InstallExecution<'db, S>
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
    pub(crate) fn new(
        cx: &ReportContext<S>,
        db: Arc<Mutex<PackageDatabase<'db>>>,
        manifest: Arc<PackageManifest>,
    ) -> Self {
        let db_guard = db_guard::begin_install(cx, db, &manifest);
        Self { db_guard, manifest }
    }

    pub(crate) async fn execute(self, cx: &ReportContext<S>) -> Result<(), S::Error> {
        let pkg_id = self.manifest.metadata.id();
        let cx =
            InstallExecutionScope::start_with_report(cx, format_args!("Installing {pkg_id}..."));

        let pkg_dirs = PackageDirs::new(cx.app_dirs(), &pkg_id);
        unregistration::unregister_package_fonts(&cx, &pkg_id)?;
        package::remove_package_dirs(&pkg_dirs)
            .context(RemovePackageFilesSnafu { pkg_id: &pkg_id })
            .report_error(cx.reporter())?;

        let pkg_dirs_guard = package_dirs_guard::create_new_package_dirs(&cx, &pkg_id)?;
        let package = stage_package(&cx, &pkg_dirs_guard, &self.manifest).await?;

        let registration_guard = registration::register_package_fonts(&cx, &package)?;

        self.db_guard.complete_install()?;

        pkg_dirs_guard.disarm();
        registration_guard.disarm();

        Ok(())
    }
}

async fn stage_package<S>(
    cx: &ReportContext<InstallExecutionScope<S>>,
    pkg_dirs: &PackageDirs,
    manifest: &PackageManifest,
) -> Result<Package, S::Error>
where
    S: ReportScope,
{
    let pkg_id = manifest.metadata.id();
    let reporter = cx.reporter();
    let package_fonts_dir = pkg_dirs.fonts_dir();

    let mut file_paths = vec![];

    for source in &manifest.sources {
        let file = cx
            .cancel_token()
            .run_until_cancelled(download::download_archive(cx, &pkg_id, source))
            .await
            .unwrap_or(Err(S::Error::cancelled()))?;

        file_paths.extend(extract::extract_archive(
            cx,
            file,
            &source.include,
            package_fonts_dir,
        )?);
    }

    let valid_entries = validate::validate_and_prune_fonts(cx, package_fonts_dir, &file_paths)?;
    if cx.cancel_token().is_cancelled() {
        return Err(S::Error::cancelled());
    }

    if valid_entries.is_empty() {
        return Err(reporter.report_error(NoValidFontsSnafu { pkg_id }.build()));
    }

    reporter.report_info(format_args!(
        "{} valid font(s) found in package",
        valid_entries.len()
    ));

    let package = Package::new(pkg_id.clone(), pkg_dirs.clone(), valid_entries);
    Ok(package)
}
