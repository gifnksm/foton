use std::{fmt::Debug, ops::Deref, sync::Arc};

use tempfile::TempDir;

use crate::{
    cli::{
        config::FotonConfig,
        context::RootContext,
        reporter::{NeverReport, OperationError, ReportScope, RootReportScope, RootReporter},
    },
    db::PackageDatabase,
    package::{
        PackageDirs, PackageId, PackageManifest, PackageName, PackageNamespace, PackageState,
        PackageVersion,
    },
    registry::{RegistryId, RegistryIndex},
    util::app_dirs::AppDirs,
};

const APP_ID: &str = "io.github.gifnksm.foton-test";

#[derive(Debug)]
pub(crate) struct TestScope {}

impl ReportScope for TestScope {
    type WarnReportValue = NeverReport;
    type ErrorReportValue = NeverReport;
    type Error = TestError;
}

impl RootReportScope for TestScope {
    fn new() -> Self {
        Self {}
    }
}

#[derive(Debug)]
pub(crate) enum TestError {
    Failed,
    Cancelled,
}

impl OperationError for TestError {
    fn failed() -> Self {
        Self::Failed
    }

    fn cancelled() -> Self {
        Self::Cancelled
    }
}

#[derive(Debug)]
pub(crate) struct TempdirContext {
    _tempdir_guard: TempDir,
    cx: RootContext,
}

impl TempdirContext {
    pub(crate) fn new() -> Self {
        Self::with_app_id(APP_ID)
    }

    pub(crate) fn with_app_id<S>(app_id: S) -> Self
    where
        S: Into<Arc<str>>,
    {
        let (tempdir, app_dirs) = make_app_dirs();
        let config = FotonConfig::default();
        let reporter = RootReporter::message_reporter();
        let cx = RootContext::new(
            app_id.into(),
            Arc::new(app_dirs),
            Arc::new(config),
            reporter,
        );
        Self {
            _tempdir_guard: tempdir,
            cx,
        }
    }
}

impl Deref for TempdirContext {
    type Target = RootContext;

    fn deref(&self) -> &Self::Target {
        &self.cx
    }
}

pub(crate) fn make_app_dirs() -> (TempDir, AppDirs) {
    let tempdir = tempfile::tempdir().unwrap();
    let app_dirs = AppDirs::new_for_test(tempdir.path()).unwrap();
    (tempdir, app_dirs)
}

pub(crate) fn make_package_dirs(pkg_id: &PackageId) -> (TempDir, AppDirs, PackageDirs) {
    let (tempdir, app_dirs) = make_app_dirs();
    let pkg_dirs = PackageDirs::new(&app_dirs, pkg_id);
    (tempdir, app_dirs, pkg_dirs)
}

pub(crate) fn make_registry() -> (TempDir, RegistryIndex) {
    let tempdir = TempDir::new().unwrap();
    let id = RegistryId::new("test-registry").unwrap();
    let registry = RegistryIndex::open(id, tempdir.path().to_path_buf()).unwrap();
    (tempdir, registry)
}

pub(crate) fn make_manifest_str<NS, N, V>(namespace: NS, name: N, version: V) -> String
where
    NS: TryInto<PackageNamespace, Error: Debug>,
    N: TryInto<PackageName, Error: Debug>,
    V: TryInto<PackageVersion, Error: Debug>,
{
    let namespace = namespace.try_into().unwrap();
    let name = name.try_into().unwrap();
    let version = version.try_into().unwrap();
    format!(
        r#"
[package]
name = "{namespace}/{name}"
version = "{version}"

[[sources]]
url = "https://example.com/{name}-{version}.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
"#
    )
}

pub(crate) fn make_manifest<NS, N, V>(namespace: NS, name: N, version: V) -> PackageManifest
where
    NS: TryInto<PackageNamespace, Error: Debug>,
    N: TryInto<PackageName, Error: Debug>,
    V: TryInto<PackageVersion, Error: Debug>,
{
    let manifest_str = make_manifest_str(namespace, name, version);
    toml::from_str(&manifest_str).unwrap()
}

pub(crate) fn mark_as_pending_installed(db: &mut PackageDatabase<'_>, manifest: &PackageManifest) {
    let pkg_id = manifest.metadata.id();
    db.begin_install(manifest).unwrap();
    assert_eq!(
        db.entry_by_id(&pkg_id).unwrap().0,
        PackageState::PendingInstall
    );
}

pub(crate) fn mark_as_installed(db: &mut PackageDatabase<'_>, manifest: &PackageManifest) {
    let pkg_id = manifest.metadata.id();
    db.begin_install(manifest).unwrap();
    db.complete_install(&pkg_id).unwrap();
    assert_eq!(db.entry_by_id(&pkg_id).unwrap().0, PackageState::Installed);
}

pub(crate) fn mark_as_pending_uninstalled(
    db: &mut PackageDatabase<'_>,
    manifest: &PackageManifest,
) {
    let pkg_id = manifest.metadata.id();
    db.begin_install(manifest).unwrap();
    db.complete_install(&pkg_id).unwrap();
    db.begin_uninstall(&pkg_id).unwrap();
    assert_eq!(
        db.entry_by_id(&pkg_id).unwrap().0,
        PackageState::PendingUninstall
    );
}
