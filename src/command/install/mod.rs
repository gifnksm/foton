use reqwest::Url;

use crate::{
    command::install::steps::{DownloadError, ExtractError, ValidationError},
    package::{self, Package, PackageDirs, PackageId, PackageManifest},
    platform::windows::services::registration::{self, RegistrationError},
    util::{
        app_dirs::AppDirs,
        fs::FsError,
        reporter::{ReportErrorExt as _, Reporter},
    },
};

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum InstallWarning {
    #[display("failed to remove package directory after install failure: {}; manual cleanup may be required", pkg_dirs.version_dir().display())]
    RemovePackageDirectoryAfterInstallFailure {
        pkg_dirs: PackageDirs,
        #[error(source)]
        source: FsError,
    },
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum InstallError {
    #[display("failed to create package directories for package {pkg_id}")]
    CreatePackageDirs {
        pkg_id: PackageId,
        #[error(source)]
        source: FsError,
    },
    #[display("failed to download package archive for package {pkg_id}: {url}")]
    Download {
        pkg_id: PackageId,
        url: Url,
        #[error(source)]
        source: Box<DownloadError>,
    },
    #[display("failed to extract package archive for package {pkg_id}")]
    Extract {
        pkg_id: PackageId,
        #[error(source)]
        source: Box<ExtractError>,
    },
    #[display("failed to validate fonts for package {pkg_id}")]
    Validation {
        pkg_id: PackageId,
        #[error(source)]
        source: Box<ValidationError>,
    },
    #[display("no valid font files found in package {pkg_id}")]
    NoValidFonts { pkg_id: PackageId },
    #[display("failed to register fonts for package {pkg_id}")]
    Registration {
        pkg_id: PackageId,
        #[error(source)]
        source: RegistrationError,
    },
}

#[derive(Debug)]
#[expect(clippy::struct_field_names)]
pub(crate) struct InstallConfig {
    pub(crate) max_archive_size_bytes: u64,
    pub(crate) max_extracted_files: usize,
    pub(crate) max_extracted_file_size_bytes: u64,
}

mod steps;

pub(crate) fn install_package(
    reporter: &mut Reporter<'_>,
    app_id: &str,
    manifest: &PackageManifest,
    app_dirs: &AppDirs,
    config: &InstallConfig,
) -> Result<Package, Box<InstallError>> {
    let pkg_id = manifest.metadata.id();
    reporter.report_step(format_args!("Installing {pkg_id}..."));

    let pkg_dirs = PackageDirs::new(app_dirs.data_dir(), &pkg_id);

    package::create_new_package_dirs(&pkg_dirs).map_err(|source| {
        let pkg_id = pkg_id.clone();
        InstallError::CreatePackageDirs { pkg_id, source }
    })?;

    let package = match stage_package(reporter, &pkg_dirs, manifest, config) {
        Ok(package) => package,
        Err(err) => {
            let _ = package::remove_package_dirs(&pkg_dirs)
                .map_err(|source| {
                    let pkg_dirs = pkg_dirs.clone();
                    InstallWarning::RemovePackageDirectoryAfterInstallFailure { pkg_dirs, source }
                })
                .report_err_as_warn(reporter);
            return Err(err);
        }
    };

    reporter.report_step(format_args!("Registering fonts..."));
    let res = registration::register_package_fonts(reporter, app_id, &package).map_err(|source| {
        let pkg_id = pkg_id.clone();
        InstallError::Registration { pkg_id, source }
    });

    if let Err(err) = res {
        let _ = registration::unregister_package_fonts(reporter, app_id, &pkg_id);
        let _ = package::remove_package_dirs(&pkg_dirs)
            .map_err(|source| {
                let pkg_dirs = pkg_dirs.clone();
                InstallWarning::RemovePackageDirectoryAfterInstallFailure { pkg_dirs, source }
            })
            .report_err_as_warn(reporter);
        return Err(err.into());
    }

    Ok(package)
}

fn stage_package(
    reporter: &mut Reporter<'_>,
    pkg_dirs: &PackageDirs,
    manifest: &PackageManifest,
    config: &InstallConfig,
) -> Result<Package, Box<InstallError>> {
    let pkg_id = manifest.metadata.id();
    let package_fonts_dir = pkg_dirs.fonts_dir();

    let mut file_paths = vec![];

    for source in &manifest.sources {
        reporter.report_step(format_args!("Downloading {}...", source.url));
        let file = steps::download_archive(reporter, &pkg_id, source, config).map_err(|err| {
            let pkg_id = pkg_id.clone();
            let url = source.url.clone();
            InstallError::Download {
                pkg_id,
                url,
                source: err,
            }
        })?;

        reporter.report_step(format_args!(
            "Extracting archive to {}...",
            package_fonts_dir.display()
        ));
        file_paths.extend(
            steps::extract_archive(file, &source.include, package_fonts_dir, config).map_err(
                |source| {
                    let pkg_id = pkg_id.clone();
                    InstallError::Extract { pkg_id, source }
                },
            )?,
        );
    }

    reporter.report_step(format_args!("Validating fonts..."));
    let valid_entries = steps::validate_and_prune_fonts(reporter, package_fonts_dir, &file_paths)
        .map_err(|source| {
        let pkg_id = pkg_id.clone();
        InstallError::Validation { pkg_id, source }
    })?;

    if valid_entries.is_empty() {
        let pkg_id = pkg_id.clone();
        return Err(InstallError::NoValidFonts { pkg_id }.into());
    }

    reporter.report_info(format_args!(
        "{} valid font(s) found in package",
        valid_entries.len()
    ));

    let package = Package::new(pkg_id, pkg_dirs.clone(), valid_entries);
    Ok(package)
}
