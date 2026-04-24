use crate::{
    package::{self, Package, PackageId},
    platform::windows::services::registration::{self, RegistrationError},
    util::{fs::FsError, reporter::Reporter},
};

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum UninstallError {
    #[display("failed to unregister fonts for package {pkg_id}")]
    UnregisterFonts {
        pkg_id: PackageId,
        #[error(source)]
        source: RegistrationError,
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

pub(crate) fn uninstall_package(
    reporter: &Reporter,
    app_id: &str,
    package: &Package,
) -> Result<(), Box<UninstallError>> {
    reporter.report_step(format_args!("Uninstalling {}...", package.id()));

    reporter.report_step(format_args!("Unregistering fonts..."));
    registration::unregister_package_fonts(reporter, app_id, package.id()).map_err(|source| {
        let pkg_id = package.id().clone();
        UninstallError::UnregisterFonts { pkg_id, source }
    })?;

    reporter.report_step(format_args!("Removing package files..."));
    package::remove_package_dirs(package.dirs()).map_err(|source| {
        let pkg_id = package.id().clone();
        UninstallError::RemovePackageFiles { pkg_id, source }
    })?;

    Ok(())
}
