use crate::{
    package::{Package, PackageDirs, PackageId, PackageManifest},
    util::{
        app_dirs::AppDirs,
        fs::FsError,
        reporter::{ReportValue, Reporter, Step, StepReporter},
    },
};

#[derive(Debug)]
struct InstallStep<'a> {
    pkg_id: &'a PackageId,
}

impl Step for InstallStep<'_> {
    type WarnReportValue = InstallWarnReport;
    type ErrorReportValue = InstallErrorReport;
    type Error = InstallError;

    fn report_prelude(&self, reporter: &Reporter) {
        reporter.report_step(format_args!("Installing {}...", self.pkg_id));
    }

    fn make_error(&self) -> Self::Error {
        InstallError {}
    }
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
enum InstallWarnReport {
    #[display("failed to remove package directory after install failure: {}; manual cleanup may be required", pkg_dirs.version_dir().display())]
    RemovePackageDirectoryAfterInstallFailure {
        pkg_dirs: PackageDirs,
        #[error(source)]
        source: FsError,
    },
}

impl From<InstallWarnReport> for ReportValue<'static> {
    fn from(report: InstallWarnReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
enum InstallErrorReport {
    #[display("failed to create package directories for package {pkg_id}")]
    CreatePackageDirs {
        pkg_id: PackageId,
        #[error(source)]
        source: FsError,
    },
    #[display("no valid font files found in package {pkg_id}")]
    NoValidFonts { pkg_id: PackageId },
}

impl From<InstallErrorReport> for ReportValue<'static> {
    fn from(report: InstallErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
#[display("failed to install package")]
pub(crate) struct InstallError {}

#[derive(Debug)]
#[expect(clippy::struct_field_names)]
pub(crate) struct InstallConfig {
    pub(crate) max_archive_size_bytes: u64,
    pub(crate) max_extracted_files: usize,
    pub(crate) max_extracted_file_size_bytes: u64,
}

mod helpers;
mod steps;

pub(crate) fn install_package(
    reporter: &Reporter,
    app_id: &str,
    manifest: &PackageManifest,
    app_dirs: &AppDirs,
    config: &InstallConfig,
) -> Result<Package, InstallError> {
    let pkg_id = manifest.metadata.id();
    let reporter = reporter.with_step(InstallStep { pkg_id: &pkg_id });

    let pkg_dirs = helpers::create_new_package_dirs(&reporter, app_dirs, &pkg_id)?;
    let package = stage_package(&reporter, &pkg_dirs, manifest, config)?;

    let registration = steps::register_package_fonts(&reporter, app_id, &package)?;

    pkg_dirs.disarm();
    registration.disarm();

    Ok(package)
}

fn stage_package(
    reporter: &StepReporter<'_, InstallStep<'_>>,
    pkg_dirs: &PackageDirs,
    manifest: &PackageManifest,
    config: &InstallConfig,
) -> Result<Package, InstallError> {
    let pkg_id = manifest.metadata.id();
    let package_fonts_dir = pkg_dirs.fonts_dir();

    let mut file_paths = vec![];

    for source in &manifest.sources {
        let file = steps::download_archive(reporter, &pkg_id, source, config)?;

        file_paths.extend(steps::extract_archive(
            reporter,
            file,
            &source.include,
            package_fonts_dir,
            config,
        )?);
    }

    let valid_entries = steps::validate_and_prune_fonts(reporter, package_fonts_dir, &file_paths)?;

    if valid_entries.is_empty() {
        let pkg_id = pkg_id.clone();
        return Err(reporter.report_error(InstallErrorReport::NoValidFonts { pkg_id }));
    }

    reporter.report_info(format_args!(
        "{} valid font(s) found in package",
        valid_entries.len()
    ));

    let package = Package::new(pkg_id, pkg_dirs.clone(), valid_entries);
    Ok(package)
}
