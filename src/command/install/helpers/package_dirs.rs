use std::ops::Deref;

use crate::{
    command::{InstallError, InstallWarning},
    package::{self, PackageDirs, PackageId},
    util::{
        app_dirs::AppDirs,
        reporter::{ReportErrorExt as _, Reporter},
    },
};

pub(crate) fn create_new_package_dirs<'a>(
    reporter: &'a Reporter,
    app_dirs: &AppDirs,
    pkg_id: &PackageId,
) -> Result<PackageDirsGuard<'a>, Box<InstallError>> {
    let pkg_dirs = PackageDirs::new(app_dirs.data_dir(), pkg_id);
    package::create_new_package_dirs(&pkg_dirs).map_err(|source| {
        let pkg_id = pkg_id.clone();
        InstallError::CreatePackageDirs { pkg_id, source }
    })?;
    Ok(PackageDirsGuard {
        armed: true,
        reporter,
        pkg_dirs,
    })
}

#[must_use]
#[derive(Debug)]
pub(crate) struct PackageDirsGuard<'a> {
    armed: bool,
    reporter: &'a Reporter,
    pkg_dirs: PackageDirs,
}

impl Deref for PackageDirsGuard<'_> {
    type Target = PackageDirs;

    fn deref(&self) -> &Self::Target {
        &self.pkg_dirs
    }
}

impl Drop for PackageDirsGuard<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }

        let _ = package::remove_package_dirs(&self.pkg_dirs)
            .map_err(|source| {
                let pkg_dirs = self.pkg_dirs.clone();
                InstallWarning::RemovePackageDirectoryAfterInstallFailure { pkg_dirs, source }
            })
            .report_err_as_warn(self.reporter);
    }
}

impl PackageDirsGuard<'_> {
    pub(crate) fn disarm(mut self) {
        self.armed = false;
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use semver::Version;
    use tempfile::TempDir;

    use crate::{
        package::{PackageName, PackageNamespace},
        util::path::AbsolutePath,
    };

    use super::*;

    fn make_package_id() -> PackageId {
        let namespace = PackageNamespace::new("yuru7").unwrap();
        let name = PackageName::new("hackgen").unwrap();
        let version = Version::new(2, 10, 0);
        PackageId::new(namespace, name, version)
    }

    fn make_package_dirs() -> (TempDir, PackageDirs) {
        let tempdir = tempfile::tempdir().unwrap();
        let app_data_dir = AbsolutePath::new(tempdir.path()).unwrap();
        let pkg_id = make_package_id();
        let pkg_dirs = PackageDirs::new(app_data_dir, &pkg_id);
        (tempdir, pkg_dirs)
    }

    #[test]
    fn create_new_package_dirs_does_not_remove_existing_package_on_failure() {
        let (tempdir, pkg_dirs) = make_package_dirs();
        fs::create_dir_all(pkg_dirs.fonts_dir()).unwrap();
        let existing_font = pkg_dirs.fonts_dir().join("existing.ttf");
        fs::write(&existing_font, b"font").unwrap();

        let reporter = Reporter::message_reporter();
        let app_dirs = AppDirs::new_for_test(AbsolutePath::new(tempdir.path()).unwrap());
        let pkg_id = make_package_id();

        let err = create_new_package_dirs(&reporter, &app_dirs, &pkg_id).unwrap_err();

        assert!(matches!(*err, InstallError::CreatePackageDirs { .. }));
        assert!(pkg_dirs.version_dir().exists());
        assert!(pkg_dirs.fonts_dir().exists());
        assert!(existing_font.exists());
    }

    #[test]
    fn package_dirs_guard_removes_created_directories_on_drop() {
        let (tempdir, pkg_dirs) = make_package_dirs();
        let reporter = Reporter::message_reporter();
        let app_dirs = AppDirs::new_for_test(AbsolutePath::new(tempdir.path()).unwrap());
        let pkg_id = make_package_id();

        {
            let _guard = create_new_package_dirs(&reporter, &app_dirs, &pkg_id).unwrap();
            assert!(pkg_dirs.fonts_dir().exists());
            assert!(pkg_dirs.version_dir().exists());
        }

        assert!(!pkg_dirs.fonts_dir().exists());
        assert!(!pkg_dirs.version_dir().exists());
        assert!(!pkg_dirs.name_dir().exists());
        assert!(!pkg_dirs.namespace_dir().exists());
    }
}
