use std::{marker::PhantomData, ops::Deref};

use snafu::{ResultExt as _, Snafu};

use crate::{
    cli::{
        context::ReportContext,
        reporter::{
            NeverReport, ReportScope, ScopeResultErrorExt as _, ScopeResultNoticeExt as _,
            SubReportScope,
        },
    },
    engine::execute::support::CleanupTracker,
    package::{self, PackageDirs, PackageId},
    util::{fs::FsError, macros::concat_line},
};

#[derive(Debug, Default)]
struct PackageDirsScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for PackageDirsScope<S>
where
    S: ReportScope,
{
    type NoticeReportValue = PackageDirNoticeReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = PackageDirErrorReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for PackageDirsScope<S> where S: ReportScope {}

#[derive(Debug, Snafu)]
enum PackageDirNoticeReport {
    #[snafu(display(
        concat_line!(
            "failed to remove package directory after install failure",
            "run `foton repair {pkg_id}` to retry cleanup",
            "if repair does not resolve the problem, manual cleanup may be required",
        ),
        pkg_id = pkg_id,
    ))]
    RemovePackageDirectoryAfterInstallFailure { pkg_id: PackageId, source: FsError },
}

#[derive(Debug, Snafu)]
enum PackageDirErrorReport {
    #[snafu(display("failed to create package directories for package {pkg_id}"))]
    CreatePackageDirs { pkg_id: PackageId, source: FsError },
}

pub(super) fn create_new_package_dirs<S>(
    cx: &ReportContext<S>,
    cleanup_tracker: CleanupTracker,
    pkg_id: &PackageId,
) -> Result<PackageDirsGuard<S>, S::Error>
where
    S: ReportScope,
{
    let cx = PackageDirsScope::start(cx);
    let reporter = cx.reporter();
    let pkg_dirs = PackageDirs::new(cx.app_dirs(), pkg_id);
    let mut guard = PackageDirsGuard {
        armed: true,
        dir_created: false,
        cx: cx.clone(),
        cleanup_tracker,
        pkg_dirs,
    };
    package::create_new_package_dirs(&guard.pkg_dirs)
        .context(CreatePackageDirsSnafu { pkg_id })
        .report_error(reporter)?;
    guard.dir_created = true;
    Ok(guard)
}

#[must_use]
#[derive(Debug)]
pub(super) struct PackageDirsGuard<S>
where
    S: ReportScope,
{
    armed: bool,
    dir_created: bool,
    cx: ReportContext<PackageDirsScope<S>>,
    cleanup_tracker: CleanupTracker,
    pkg_dirs: PackageDirs,
}

impl<S> Deref for PackageDirsGuard<S>
where
    S: ReportScope,
{
    type Target = PackageDirs;

    fn deref(&self) -> &Self::Target {
        &self.pkg_dirs
    }
}

impl<S> Drop for PackageDirsGuard<S>
where
    S: ReportScope,
{
    fn drop(&mut self) {
        if !self.armed {
            return;
        }

        if !self.dir_created {
            // Directory creation failed before this install attempt fully owned the
            // package directory. Avoid removing pre-existing contents here, but
            // keep the DB entry in a cleanup-required state.
            self.cleanup_tracker.request_cleanup();
            return;
        }

        self.cx
            .reporter()
            .report_info(format_args!("rolling back package fonts directories..."));

        self.cleanup_tracker
            .do_rollback_cleanup(|| package::remove_package_dirs(&self.pkg_dirs))
            .context(RemovePackageDirectoryAfterInstallFailureSnafu {
                pkg_id: self.pkg_dirs.pkg_id().clone(),
            })
            .report_notice(self.cx.reporter());
    }
}

impl<S> PackageDirsGuard<S>
where
    S: ReportScope,
{
    pub(super) fn disarm(mut self) {
        self.armed = false;
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, sync::LazyLock};

    use super::*;
    use crate::{package::PackageId, util::testing};

    static PKG_ID: LazyLock<PackageId> = LazyLock::new(|| "example-font@0.1.0".parse().unwrap());

    #[test]
    fn create_new_package_dirs_does_not_remove_existing_package_on_failure() {
        testing::with_context(|cx| {
            let cleanup_tracker = CleanupTracker::default();

            let pkg_dirs = PackageDirs::new(cx.app_dirs(), &PKG_ID);
            fs::create_dir_all(pkg_dirs.fonts_dir()).unwrap();
            let existing_font = pkg_dirs.fonts_dir().join("existing.ttf");
            fs::write(&existing_font, b"font").unwrap();

            create_new_package_dirs(cx, cleanup_tracker.clone(), &PKG_ID).unwrap_err();

            assert!(cleanup_tracker.cleanup_required());
            assert!(pkg_dirs.version_dir().exists());
            assert!(pkg_dirs.fonts_dir().exists());
            assert!(existing_font.exists());
        });
    }

    #[test]
    fn package_dirs_guard_removes_created_directories_on_drop() {
        testing::with_context(|cx| {
            let cleanup_tracker = CleanupTracker::default();
            let pkg_dirs = PackageDirs::new(cx.app_dirs(), &PKG_ID);

            {
                let _guard = create_new_package_dirs(cx, cleanup_tracker.clone(), &PKG_ID).unwrap();
                assert!(pkg_dirs.fonts_dir().exists());
                assert!(pkg_dirs.version_dir().exists());
            }

            assert!(!cleanup_tracker.cleanup_required());
            assert!(!pkg_dirs.fonts_dir().exists());
            assert!(!pkg_dirs.version_dir().exists());
            assert!(!pkg_dirs.name_dir().exists());
        });
    }

    #[test]
    fn package_dirs_guard_marks_cleanup_failed_when_drop_cleanup_fails() {
        testing::with_context(|cx| {
            let cleanup_tracker = CleanupTracker::default();
            let pkg_dirs = PackageDirs::new(cx.app_dirs(), &PKG_ID);

            {
                let _guard = create_new_package_dirs(cx, cleanup_tracker.clone(), &PKG_ID).unwrap();
                fs::write(pkg_dirs.version_dir().join("leftover.txt"), b"leftover").unwrap();
            }

            assert!(cleanup_tracker.cleanup_required());
            assert!(pkg_dirs.version_dir().exists());
            assert!(pkg_dirs.name_dir().exists());
        });
    }
}
