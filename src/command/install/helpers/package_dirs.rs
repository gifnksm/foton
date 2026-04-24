use std::ops::Deref;

use crate::{
    command::{
        InstallError,
        install::{InstallErrorReport, InstallStep, InstallWarnReport},
    },
    package::{self, PackageDirs, PackageId},
    util::{
        app_dirs::AppDirs,
        reporter::{StepReporter, StepResultErrorExt as _, StepResultWarnExt as _},
    },
};

pub(in crate::command::install) fn create_new_package_dirs<'a, 'b, 'c>(
    reporter: &'a StepReporter<'b, InstallStep<'c>>,
    app_dirs: &AppDirs,
    pkg_id: &PackageId,
) -> Result<PackageDirsGuard<'a, 'b, 'c>, InstallError> {
    let pkg_dirs = PackageDirs::new(app_dirs.data_dir(), pkg_id);
    package::create_new_package_dirs(&pkg_dirs)
        .map_err(|source| {
            let pkg_id = pkg_id.clone();
            InstallErrorReport::CreatePackageDirs { pkg_id, source }
        })
        .report_error(reporter)?;
    Ok(PackageDirsGuard {
        armed: true,
        reporter,
        pkg_dirs,
    })
}

#[must_use]
#[derive(Debug)]
pub(in crate::command::install) struct PackageDirsGuard<'a, 'b, 'c> {
    armed: bool,
    reporter: &'a StepReporter<'b, InstallStep<'c>>,
    pkg_dirs: PackageDirs,
}

impl Deref for PackageDirsGuard<'_, '_, '_> {
    type Target = PackageDirs;

    fn deref(&self) -> &Self::Target {
        &self.pkg_dirs
    }
}

impl Drop for PackageDirsGuard<'_, '_, '_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }

        self.reporter
            .report_info(format_args!("rolling back package fonts directories..."));

        let _ = package::remove_package_dirs(&self.pkg_dirs)
            .map_err(|source| {
                let pkg_dirs = self.pkg_dirs.clone();
                InstallWarnReport::RemovePackageDirectoryAfterInstallFailure { pkg_dirs, source }
            })
            .report_warn(self.reporter);
    }
}

impl PackageDirsGuard<'_, '_, '_> {
    pub(in crate::command::install) fn disarm(mut self) {
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
        util::{path::AbsolutePath, reporter::Reporter},
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

        let pkg_id = make_package_id();
        let reporter = Reporter::message_reporter();
        let reporter = reporter.with_step(InstallStep { pkg_id: &pkg_id });
        let app_dirs = AppDirs::new_for_test(AbsolutePath::new(tempdir.path()).unwrap());

        create_new_package_dirs(&reporter, &app_dirs, &pkg_id).unwrap_err();

        assert!(pkg_dirs.version_dir().exists());
        assert!(pkg_dirs.fonts_dir().exists());
        assert!(existing_font.exists());
    }

    #[test]
    fn package_dirs_guard_removes_created_directories_on_drop() {
        let (tempdir, pkg_dirs) = make_package_dirs();
        let pkg_id = make_package_id();
        let reporter = Reporter::message_reporter();
        let reporter = reporter.with_step(InstallStep { pkg_id: &pkg_id });
        let app_dirs = AppDirs::new_for_test(AbsolutePath::new(tempdir.path()).unwrap());

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
