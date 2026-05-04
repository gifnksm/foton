use std::sync::{Arc, Mutex};

use snafu::Snafu;

use crate::{
    cli::{
        context::ReportContext,
        reporter::{NeverReport, ReportScope, ReportValue, SubReportScope},
    },
    db::PackageDatabase,
    package::{Package, PackageDirs, PackageId, PackageManifest},
};

mod db_guard;
mod download;
mod extract;
mod package_dirs_guard;
mod registration;
mod validate;

#[derive(Debug)]
struct InstallExecutionScope<S> {
    base_scope: Arc<S>,
}

impl<S> ReportScope for InstallExecutionScope<S>
where
    S: ReportScope,
{
    type WarnReportValue = NeverReport;
    type ErrorReportValue = InstallExecutionErrorReport;
    type Error = S::Error;

    fn make_failed(&self) -> Self::Error {
        self.base_scope.make_failed()
    }

    fn make_cancelled(&self) -> Self::Error {
        self.base_scope.make_cancelled()
    }
}

impl<S> SubReportScope<S> for InstallExecutionScope<S>
where
    S: ReportScope,
{
    fn new(base_scope: Arc<S>) -> Self {
        Self { base_scope }
    }
}

impl From<InstallExecutionErrorReport> for ReportValue<'static> {
    fn from(report: InstallExecutionErrorReport) -> Self {
        Self::BoxedError(report.into())
    }
}

#[derive(Debug, Snafu)]
enum InstallExecutionErrorReport {
    #[snafu(display("no valid font files found in package {pkg_id}"))]
    NoValidFonts { pkg_id: PackageId },
}

pub(crate) async fn execute_install<S>(
    cx: &ReportContext<S>,
    db: &Arc<Mutex<PackageDatabase<'_>>>,
    manifest: &PackageManifest,
) -> Result<(), S::Error>
where
    S: ReportScope,
{
    let cx = InstallExecutionScope::start(cx);

    let db = db_guard::begin_install(&cx, Arc::clone(db), manifest)?;

    let pkg_id = manifest.metadata.id();
    let pkg_dirs = package_dirs_guard::create_new_package_dirs(&cx, &pkg_id)?;
    let package = stage_package(&cx, &pkg_dirs, manifest).await?;

    let registration = registration::register_package_fonts(&cx, &package)?;

    db.complete_install()?;

    pkg_dirs.disarm();
    registration.disarm();

    Ok(())
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
            .unwrap_or(Err(cx.scope().make_cancelled()))?;

        file_paths.extend(extract::extract_archive(
            cx,
            file,
            &source.include,
            package_fonts_dir,
        )?);
    }

    let valid_entries = validate::validate_and_prune_fonts(cx, package_fonts_dir, &file_paths)?;
    if cx.cancel_token().is_cancelled() {
        return Err(cx.scope().make_cancelled());
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
