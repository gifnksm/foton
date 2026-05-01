use std::{ops::Deref, sync::Arc};

use snafu::{ResultExt as _, Snafu};

use crate::{
    cli::{
        context::ReportContext,
        reporter::{
            ReportScope, ReportValue, ScopeResultErrorExt as _, ScopeResultWarnExt as _,
            SubReportScope,
        },
    },
    package::{self, PackageDirs, PackageId},
    util::fs::FsError,
};

#[derive(Debug)]
struct PackageDirsScope<S> {
    base_scope: Arc<S>,
}

impl<S> ReportScope for PackageDirsScope<S>
where
    S: ReportScope,
{
    type WarnReportValue = PackageDirWarnReport;
    type ErrorReportValue = PackageDirErrorReport;
    type Error = S::Error;

    fn make_failed(&self) -> Self::Error {
        self.base_scope.make_failed()
    }
}

impl<S> SubReportScope<S> for PackageDirsScope<S>
where
    S: ReportScope,
{
    fn new(base_scope: Arc<S>) -> Self {
        Self { base_scope }
    }
}

#[derive(Debug, Snafu)]
enum PackageDirWarnReport {
    #[snafu(display(
        "failed to remove package directory after install failure\nmanual cleanup may be required"
    ))]
    RemovePackageDirectoryAfterInstallFailure { source: FsError },
}

impl From<PackageDirWarnReport> for ReportValue<'static> {
    fn from(report: PackageDirWarnReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

#[derive(Debug, Snafu)]
enum PackageDirErrorReport {
    #[snafu(display("failed to create package directories for package {pkg_id}"))]
    CreatePackageDirs { pkg_id: PackageId, source: FsError },
}

impl From<PackageDirErrorReport> for ReportValue<'static> {
    fn from(report: PackageDirErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

pub(in crate::command::install) fn create_new_package_dirs<S>(
    cx: &ReportContext<S>,
    pkg_id: &PackageId,
) -> Result<PackageDirsGuard<S>, S::Error>
where
    S: ReportScope,
{
    let cx = PackageDirsScope::start(cx);
    let reporter = cx.reporter();
    let pkg_dirs = PackageDirs::new(cx.app_dirs(), pkg_id);
    package::create_new_package_dirs(&pkg_dirs)
        .context(CreatePackageDirsSnafu { pkg_id })
        .report_error(reporter)?;
    Ok(PackageDirsGuard {
        armed: true,
        cx: cx.clone(),
        pkg_dirs,
    })
}

#[must_use]
#[derive(Debug)]
pub(in crate::command::install) struct PackageDirsGuard<S>
where
    S: ReportScope,
{
    armed: bool,
    cx: ReportContext<PackageDirsScope<S>>,
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

        self.cx
            .reporter()
            .report_info(format_args!("rolling back package fonts directories..."));

        let _ = package::remove_package_dirs(&self.pkg_dirs)
            .context(RemovePackageDirectoryAfterInstallFailureSnafu)
            .report_warn(self.cx.reporter());
    }
}

impl<S> PackageDirsGuard<S>
where
    S: ReportScope,
{
    pub(in crate::command::install) fn disarm(mut self) {
        self.armed = false;
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, sync::LazyLock};

    use super::*;
    use crate::{
        cli::reporter::RootReportScope as _,
        package::PackageId,
        util::testing::{TempdirContext, TestScope},
    };

    static PKG_ID: LazyLock<PackageId> =
        LazyLock::new(|| "example-namespace/example-font@0.1.0".parse().unwrap());

    #[test]
    fn create_new_package_dirs_does_not_remove_existing_package_on_failure() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let pkg_dirs = PackageDirs::new(cx.app_dirs(), &PKG_ID);
        fs::create_dir_all(pkg_dirs.fonts_dir()).unwrap();
        let existing_font = pkg_dirs.fonts_dir().join("existing.ttf");
        fs::write(&existing_font, b"font").unwrap();

        create_new_package_dirs(&cx, &PKG_ID).unwrap_err();

        assert!(pkg_dirs.version_dir().exists());
        assert!(pkg_dirs.fonts_dir().exists());
        assert!(existing_font.exists());
    }

    #[test]
    fn package_dirs_guard_removes_created_directories_on_drop() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let pkg_dirs = PackageDirs::new(cx.app_dirs(), &PKG_ID);

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
