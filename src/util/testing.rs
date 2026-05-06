use std::{fmt::Debug, ops::Deref, sync::Arc};

use tempfile::TempDir;

use crate::{
    cli::{
        config::FotonConfig,
        context::{ReportContext, RootContext},
        reporter::{NeverReport, OperationError, ReportScope, RootReportScope, RootReporter},
    },
    db::PackageDatabase,
    engine,
    package::{PackageDirs, PackageId, PackageManifest, PackageState},
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

pub(crate) fn make_manifest_str<I>(pkg_id: I) -> String
where
    I: TryInto<PackageId, Error: Debug>,
{
    let pkg_id = pkg_id.try_into().unwrap();
    let namespace = pkg_id.namespace();
    let name = pkg_id.name();
    let version = pkg_id.version();
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

pub(crate) fn make_manifest<I>(pkg_id: I) -> Arc<PackageManifest>
where
    I: TryInto<PackageId, Error: Debug>,
{
    let manifest_str = make_manifest_str(pkg_id);
    Arc::new(toml::from_str(&manifest_str).unwrap())
}

pub(crate) fn mark_as_pending_installed(
    db: &mut PackageDatabase<'_>,
    manifest: &Arc<PackageManifest>,
) {
    let pkg_id = manifest.metadata.id();
    db.apply_install_transaction(manifest).unwrap();
    assert_eq!(
        db.entry_by_id(&pkg_id).unwrap().0,
        PackageState::PendingInstall
    );
}

pub(crate) fn mark_as_installed(db: &mut PackageDatabase<'_>, manifest: &Arc<PackageManifest>) {
    let pkg_id = manifest.metadata.id();
    db.apply_install_transaction(manifest).unwrap();
    db.complete_install(&pkg_id).unwrap();
    assert_eq!(db.entry_by_id(&pkg_id).unwrap().0, PackageState::Installed);
}

pub(crate) fn mark_as_pending_uninstalled(
    db: &mut PackageDatabase<'_>,
    manifest: &Arc<PackageManifest>,
) {
    let pkg_id = manifest.metadata.id();
    db.apply_install_transaction(manifest).unwrap();
    db.complete_install(&pkg_id).unwrap();
    db.apply_uninstall_transaction(&pkg_id).unwrap();
    assert_eq!(
        db.entry_by_id(&pkg_id).unwrap().0,
        PackageState::PendingUninstall
    );
}

pub(crate) fn with_db<S, F>(cx: &ReportContext<S>, f: F)
where
    S: ReportScope,
    F: FnOnce(PackageDatabase<'_>),
{
    let mut lock_file = engine::open_db_lock_file(cx).unwrap();
    let db = engine::load_database(cx, &mut lock_file).unwrap();
    f(db);
}
