use std::ops::Deref;

use crate::{
    cli::context::StepContext,
    command::{
        InstallError,
        install::{InstallErrorReport, InstallStep, InstallWarnReport},
    },
    package::{self, PackageDirs, PackageId},
    util::reporter::{StepResultErrorExt as _, StepResultWarnExt as _},
};

pub(in crate::command::install) fn create_new_package_dirs(
    cx: &StepContext<InstallStep>,
    pkg_id: &PackageId,
) -> Result<PackageDirsGuard, InstallError> {
    let reporter = cx.reporter();
    let pkg_dirs = PackageDirs::new(cx.app_dirs(), pkg_id);
    package::create_new_package_dirs(&pkg_dirs)
        .map_err(|source| {
            let pkg_id = pkg_id.clone();
            InstallErrorReport::CreatePackageDirs { pkg_id, source }
        })
        .report_error(reporter)?;
    Ok(PackageDirsGuard {
        armed: true,
        cx: cx.clone(),
        pkg_dirs,
    })
}

#[must_use]
#[derive(Debug)]
pub(in crate::command::install) struct PackageDirsGuard {
    armed: bool,
    cx: StepContext<InstallStep>,
    pkg_dirs: PackageDirs,
}

impl Deref for PackageDirsGuard {
    type Target = PackageDirs;

    fn deref(&self) -> &Self::Target {
        &self.pkg_dirs
    }
}

impl Drop for PackageDirsGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }

        self.cx
            .reporter()
            .report_info(format_args!("rolling back package fonts directories..."));

        let _ = package::remove_package_dirs(&self.pkg_dirs)
            .map_err(|source| {
                let pkg_dirs = self.pkg_dirs.clone();
                InstallWarnReport::RemovePackageDirectoryAfterInstallFailure { pkg_dirs, source }
            })
            .report_warn(self.cx.reporter());
    }
}

impl PackageDirsGuard {
    pub(in crate::command::install) fn disarm(mut self) {
        self.armed = false;
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, sync::LazyLock};

    use crate::{package::PackageId, util::testing::TempdirContext};

    use super::*;

    static PKG_ID: LazyLock<PackageId> =
        LazyLock::new(|| "example-namespace/example-font@0.1.0".parse().unwrap());

    #[test]
    fn create_new_package_dirs_does_not_remove_existing_package_on_failure() {
        let cx = TempdirContext::new();
        let pkg_dirs = PackageDirs::new(cx.app_dirs(), &PKG_ID);
        fs::create_dir_all(pkg_dirs.fonts_dir()).unwrap();
        let existing_font = pkg_dirs.fonts_dir().join("existing.ttf");
        fs::write(&existing_font, b"font").unwrap();

        let cx = cx.with_step(InstallStep {});

        create_new_package_dirs(&cx, &PKG_ID).unwrap_err();

        assert!(pkg_dirs.version_dir().exists());
        assert!(pkg_dirs.fonts_dir().exists());
        assert!(existing_font.exists());
    }

    #[test]
    fn package_dirs_guard_removes_created_directories_on_drop() {
        let cx = TempdirContext::new();
        let pkg_dirs = PackageDirs::new(cx.app_dirs(), &PKG_ID);
        let cx = cx.with_step(InstallStep {});

        {
            let _guard = create_new_package_dirs(&cx, &PKG_ID).unwrap();
            assert!(pkg_dirs.fonts_dir().exists());
            assert!(pkg_dirs.version_dir().exists());
        }

        assert!(!pkg_dirs.fonts_dir().exists());
        assert!(!pkg_dirs.version_dir().exists());
        assert!(!pkg_dirs.name_dir().exists());
        assert!(!pkg_dirs.namespace_dir().exists());
    }
}
