use std::{
    fmt::Debug,
    fs,
    ops::Deref,
    path::Path,
    process,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use snafu::Snafu;
use tempfile::TempDir;

use crate::{
    cli::{
        args::GlobalArgs,
        config::FotonConfig,
        context::{ReportContext, RootContext},
        reporter::{
            NeverReport, OperationError, ReportScope, RootReportScope, RootReporter, SubReportScope,
        },
    },
    db::PackageDatabase,
    engine,
    package::{InstallationState, PackageDefinition, PackageDirs, PackageId, PackageManifest},
    registry::{RegistryId, RegistryIndex, RegistrySource, RegistrySpec},
    util::{app_dirs::AppDirs, path::AbsolutePath},
};

#[derive(Debug, Default)]
pub(crate) struct TestScope {}

impl ReportScope for TestScope {
    type NoticeReportValue = NeverReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = NeverReport;
    type Error = TestError;
}

impl RootReportScope for TestScope {}

#[derive(Debug, derive_more::IsVariant, Snafu)]
pub(crate) enum TestError {
    #[snafu(display("test operation failed"))]
    Failed,
    #[snafu(display("test operation cancelled"))]
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
struct TempdirContext {
    _tempdir_guard: TempDir,
    cx: RootContext,
}

impl TempdirContext {
    fn new() -> Self {
        static TEST_ID: AtomicUsize = AtomicUsize::new(0);
        let app_id = format!(
            "io.github.gifnksm.foton.test.{}.{}",
            process::id(),
            TEST_ID.fetch_add(1, Ordering::Relaxed)
        );
        Self::with_app_id(app_id)
    }

    fn with_app_id<S>(app_id: S) -> Self
    where
        S: Into<Arc<str>>,
    {
        let warnings_as_errors = false;
        let (tempdir, app_dirs) = make_app_dirs();
        let config = FotonConfig::default();
        let reporter = RootReporter::message_reporter(warnings_as_errors);
        let cx = RootContext::new(
            app_id.into(),
            Arc::new(app_dirs),
            Arc::new(GlobalArgs::default()),
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

pub(crate) fn make_registry_spec<I>(id: I) -> (TempDir, RegistrySpec)
where
    I: TryInto<RegistryId, Error: Debug>,
{
    let tempdir = TempDir::new().unwrap();
    let registry = make_registry_spec_at(id, tempdir.path());
    (tempdir, registry)
}

pub(crate) fn make_registry_spec_at<I, P>(id: I, path: P) -> RegistrySpec
where
    I: TryInto<RegistryId, Error: Debug>,
    P: TryInto<AbsolutePath, Error: Debug>,
{
    let id = id.try_into().unwrap();
    let path = path.try_into().unwrap();
    let source = RegistrySource::Local(path);
    RegistrySpec::new(id, source)
}

pub(crate) fn make_registry_index<I>(id: I) -> (TempDir, RegistryIndex)
where
    I: TryInto<RegistryId, Error: Debug>,
{
    let tempdir = TempDir::new().unwrap();
    let id = id.try_into().unwrap();
    let registry = RegistryIndex::open(id, tempdir.path().to_path_buf()).unwrap();
    (tempdir, registry)
}

pub(crate) fn make_manifest_str<I>(pkg_id: I) -> String
where
    I: TryInto<PackageId, Error: Debug>,
{
    let pkg_id = pkg_id.try_into().unwrap();
    let name = pkg_id.name();
    let version = pkg_id.version();
    format!(
        r#"
name = "{name}"
version = "{version}"

[[sources]]
url = "https://example.com/{name}-{version}.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

[sources.contents]
type = "archive"
"#
    )
}

pub(crate) fn make_manifest<I>(pkg_id: I) -> PackageManifest
where
    I: TryInto<PackageId, Error: Debug>,
{
    let manifest_str = make_manifest_str(pkg_id);
    parse_manifest(&manifest_str)
}

pub(crate) fn try_parse_manifest(manifest_str: &str) -> Result<PackageManifest, toml::de::Error> {
    toml::from_str(manifest_str)
}

pub(crate) fn parse_manifest(manifest_str: &str) -> PackageManifest {
    try_parse_manifest(manifest_str).unwrap()
}

pub(crate) fn parse_manifest_to_definition(manifest_str: &str) -> PackageDefinition {
    parse_manifest(manifest_str).into()
}

pub(crate) fn make_package_definition<I>(pkg_id: I) -> PackageDefinition
where
    I: TryInto<PackageId, Error: Debug>,
{
    make_manifest(pkg_id).into()
}

pub(crate) fn make_package_definition_with_display_name<I, D>(
    pkg_id: I,
    display_name: D,
) -> PackageDefinition
where
    I: TryInto<PackageId, Error: Debug>,
    D: Into<String>,
{
    let mut manifest = make_manifest(pkg_id);
    manifest.display_name = Some(display_name.into());
    manifest.into()
}

pub(crate) fn write_manifest<P, I>(registry_dir: P, pkg_id: I)
where
    P: AsRef<Path>,
    I: TryInto<PackageId, Error: Debug>,
{
    let pkg_id = pkg_id.try_into().unwrap();
    write_manifest_str(registry_dir, &pkg_id, make_manifest_str(&pkg_id));
}

pub(crate) fn write_manifest_str<P, I, C>(registry_dir: P, pkg_id: I, manifest_str: C)
where
    P: AsRef<Path>,
    I: TryInto<PackageId, Error: Debug>,
    C: AsRef<[u8]>,
{
    let registry_dir = registry_dir.as_ref();
    let pkg_id = pkg_id.try_into().unwrap();
    let dir = registry_dir
        .join("packages")
        .join(pkg_id.name())
        .join(pkg_id.version().to_string());
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("manifest.toml"), manifest_str).unwrap();
}

pub(crate) fn mark_as_state(
    db: &mut PackageDatabase<'_>,
    pkg: &PackageDefinition,
    state: InstallationState,
) {
    match state {
        InstallationState::Installed => mark_as_installed(db, pkg),
        InstallationState::IncompleteInstall => mark_as_incomplete_install(db, pkg),
        InstallationState::IncompleteUninstall => mark_as_incomplete_uninstall(db, pkg),
    }
}

pub(crate) fn mark_as_incomplete_install(db: &mut PackageDatabase<'_>, pkg: &PackageDefinition) {
    let mut tx = db.transaction();
    tx.begin_install(pkg);
    tx.commit().unwrap();
    let entry = db.entry_by_id(&pkg.id).unwrap();
    assert!(entry.installation_state().is_incomplete_install());
    assert!(entry.activation_state().is_inactive());
}

pub(crate) fn mark_as_installed(db: &mut PackageDatabase<'_>, pkg: &PackageDefinition) {
    let mut tx = db.transaction();
    tx.begin_install(pkg);
    tx.complete_install(&pkg.id, &[]).unwrap();
    tx.commit().unwrap();

    let entry = db.entry_by_id(&pkg.id).unwrap();
    assert!(entry.installation_state().is_installed());
    assert!(entry.activation_state().is_inactive());
}

pub(crate) fn mark_as_incomplete_activation(db: &mut PackageDatabase<'_>, pkg: &PackageDefinition) {
    let mut tx = db.transaction();
    tx.begin_install(pkg);
    tx.complete_install(&pkg.id, &[]).unwrap();
    tx.begin_activate(&pkg.id).unwrap();
    tx.commit().unwrap();

    let entry = db.entry_by_id(&pkg.id).unwrap();
    assert!(entry.installation_state().is_installed());
    assert!(entry.activation_state().is_incomplete_activation());
}

pub(crate) fn mark_as_active(db: &mut PackageDatabase<'_>, pkg: &PackageDefinition) {
    let mut tx = db.transaction();
    tx.begin_install(pkg);
    tx.complete_install(&pkg.id, &[]).unwrap();
    tx.begin_activate(&pkg.id).unwrap();
    tx.complete_activate(&pkg.id).unwrap();
    tx.commit().unwrap();

    let entry = db.entry_by_id(&pkg.id).unwrap();
    assert!(entry.installation_state().is_installed());
    assert!(entry.activation_state().is_active());
}

pub(crate) fn mark_as_incomplete_deactivation(
    db: &mut PackageDatabase<'_>,
    pkg: &PackageDefinition,
) {
    let mut tx = db.transaction();
    tx.begin_install(pkg);
    tx.complete_install(&pkg.id, &[]).unwrap();
    tx.begin_activate(&pkg.id).unwrap();
    tx.complete_activate(&pkg.id).unwrap();
    tx.begin_deactivate(&pkg.id).unwrap();
    tx.commit().unwrap();

    let entry = db.entry_by_id(&pkg.id).unwrap();
    assert!(entry.installation_state().is_installed());
    assert!(entry.activation_state().is_incomplete_deactivation());
}

pub(crate) fn mark_as_incomplete_uninstall(db: &mut PackageDatabase<'_>, pkg: &PackageDefinition) {
    let mut tx = db.transaction();
    tx.begin_install(pkg);
    tx.complete_install(&pkg.id, &[]).unwrap();
    tx.begin_uninstall(&pkg.id).unwrap();
    tx.commit().unwrap();

    let entry = db.entry_by_id(&pkg.id).unwrap();
    assert!(entry.installation_state().is_incomplete_uninstall());
    assert!(entry.activation_state().is_inactive());
}

pub(crate) fn with_context<F, T>(f: F) -> T
where
    F: FnOnce(&ReportContext<TestScope>) -> T,
{
    let cx = TempdirContext::new();
    let cx = TestScope::start(&cx);
    f(&cx)
}

pub(crate) fn with_scoped_context<S, F, T>(f: F) -> T
where
    S: SubReportScope<TestScope, Error = TestError>,
    F: FnOnce(&ReportContext<S>) -> T,
{
    with_context(|cx| {
        let cx = S::start(cx);
        f(&cx)
    })
}

fn with_db_in_context<S, F, T>(cx: &ReportContext<S>, f: F) -> T
where
    S: ReportScope,
    F: FnOnce(&mut PackageDatabase<'_>) -> T,
{
    let mut lock_file = engine::open_db_lock_file(cx).unwrap();
    let exit_on_lock = true;
    let mut db = engine::load_database(cx, &mut lock_file, exit_on_lock).unwrap();
    f(&mut db)
}

pub(crate) fn with_db<F, T>(f: F) -> T
where
    F: FnOnce(&ReportContext<TestScope>, &mut PackageDatabase<'_>) -> T,
{
    with_context(|cx| with_db_in_context(cx, |db| f(cx, db)))
}

pub(crate) fn with_scoped_db<S, F, T>(f: F) -> T
where
    S: SubReportScope<TestScope, Error = TestError>,
    F: FnOnce(&ReportContext<S>, &mut PackageDatabase<'_>) -> T,
{
    with_scoped_context(|cx| with_db_in_context(cx, |db| f(cx, db)))
}
