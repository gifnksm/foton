use std::{fmt::Debug, fs, ops::Deref, sync::Arc};

use tempfile::TempDir;

use crate::{
    cli::{config::FotonConfig, context::RootContext},
    package::{
        PackageDirs, PackageId, PackageManifest, PackageName, PackageNamespace, PackageVersion,
    },
    registry::PackageRegistry,
    util::{
        app_dirs::AppDirs,
        path::AbsolutePath,
        reporter::{NeverReport, RootReporter, Step},
    },
};

const APP_ID: &str = "io.github.gifnksm.foton-test";

#[derive(Debug)]
pub(crate) struct TestStep {}

impl Step for TestStep {
    type WarnReportValue = NeverReport;
    type ErrorReportValue = NeverReport;
    type Error = TestError;

    fn make_failed(&self) -> Self::Error {
        TestError::Failed
    }
}

#[derive(Debug)]
pub(crate) enum TestError {
    Failed,
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
    let data_local_dir = AbsolutePath::new(tempdir.path()).unwrap().join("data");
    let config_dir = AbsolutePath::new(tempdir.path()).unwrap().join("config");
    fs::create_dir_all(&data_local_dir).unwrap();
    fs::create_dir_all(&config_dir).unwrap();
    let app_dirs = AppDirs::new_for_test(data_local_dir, config_dir);
    (tempdir, app_dirs)
}

pub(crate) fn make_package_dirs(pkg_id: &PackageId) -> (TempDir, AppDirs, PackageDirs) {
    let (tempdir, app_dirs) = make_app_dirs();
    let pkg_dirs = PackageDirs::new(&app_dirs, pkg_id);
    (tempdir, app_dirs, pkg_dirs)
}

pub(crate) fn make_registry() -> (TempDir, PackageRegistry) {
    let tempdir = TempDir::new().unwrap();
    let registry = PackageRegistry::new(tempdir.path().to_path_buf());
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
