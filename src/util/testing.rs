use std::{fmt::Debug, fs, ops::Deref, path::Path, sync::Arc};

use tempfile::TempDir;

use crate::{
    cli::{
        args::GlobalArgs,
        config::FotonConfig,
        context::{ReportContext, RootContext},
        reporter::{NeverReport, OperationError, ReportScope, RootReportScope, RootReporter},
    },
    db::PackageDatabase,
    engine::{
        self, ExecutionCondition, ExecutionPlan, ExecutionPlanOp, InstallOp, InstallReason,
        InstallTargetSource, ResolvedInstallTarget, ResolvedUninstallTarget, SkipOp, UninstallOp,
        UninstallReason,
    },
    package::{PackageDirs, PackageId, PackageManifest, PackageState},
    registry::{RegistryId, RegistryIndex, RegistrySource, RegistrySpec},
    util::{app_dirs::AppDirs, path::AbsolutePath},
};

const APP_ID: &str = "io.github.gifnksm.foton-test";

#[derive(Debug)]
pub(crate) struct TestScope {}

impl ReportScope for TestScope {
    type NoticeReportValue = NeverReport;
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
        .join(pkg_id.name())
        .join(pkg_id.version().to_string());
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("manifest.toml"), manifest_str).unwrap();
}

pub(crate) fn make_resolved_install_target(
    manifest: &Arc<PackageManifest>,
) -> ResolvedInstallTarget {
    ResolvedInstallTarget {
        source: InstallTargetSource::Installed,
        manifest: Arc::clone(manifest),
    }
}

pub(crate) fn make_resolved_uninstall_target<I>(pkg_id: I) -> ResolvedUninstallTarget
where
    I: TryInto<PackageId, Error: Debug>,
{
    ResolvedUninstallTarget::Resolved {
        pkg_id: pkg_id.try_into().unwrap(),
    }
}

pub(crate) fn make_install_plan(
    manifest: &Arc<PackageManifest>,
    replacing_pkg_ids: Vec<PackageId>,
) -> ExecutionPlan {
    ExecutionPlan::new_for_test([InstallOp {
        manifest: Arc::clone(manifest),
        reason: InstallReason::RequestedByUser,
        replacing_pkg_ids,
    }
    .into()])
}

pub(crate) fn make_uninstall_plan(
    pkg_id: &PackageId,
    conditions: Vec<ExecutionCondition>,
) -> ExecutionPlan {
    ExecutionPlan::new_for_test([UninstallOp {
        pkg_id: pkg_id.clone(),
        reason: UninstallReason::RequestedByUser,
        conditions,
    }
    .into()])
}

pub(crate) fn mark_as_state(
    db: &mut PackageDatabase<'_>,
    manifest: &Arc<PackageManifest>,
    state: PackageState,
) {
    match state {
        PackageState::Installed => mark_as_installed(db, manifest),
        PackageState::PendingInstall => mark_as_pending_installed(db, manifest),
        PackageState::PendingUninstall => mark_as_pending_uninstalled(db, manifest),
    }
}

pub(crate) fn mark_as_pending_installed(
    db: &mut PackageDatabase<'_>,
    manifest: &Arc<PackageManifest>,
) {
    let pkg_id = manifest.id();
    let plan = make_install_plan(manifest, vec![]);
    db.apply_plan_transaction(&plan).unwrap();
    assert_eq!(
        db.entry_by_id(&pkg_id).unwrap().0,
        PackageState::PendingInstall
    );
}

pub(crate) fn mark_as_installed(db: &mut PackageDatabase<'_>, manifest: &Arc<PackageManifest>) {
    let pkg_id = manifest.id();
    let plan = make_install_plan(manifest, vec![]);
    db.apply_plan_transaction(&plan).unwrap();
    db.complete_install(&pkg_id, &[]).unwrap();
    assert_eq!(db.entry_by_id(&pkg_id).unwrap().0, PackageState::Installed);
}

pub(crate) fn mark_as_pending_uninstalled(
    db: &mut PackageDatabase<'_>,
    manifest: &Arc<PackageManifest>,
) {
    let pkg_id = manifest.id();
    let plan = make_install_plan(manifest, vec![]);
    db.apply_plan_transaction(&plan).unwrap();
    db.complete_install(&pkg_id, &[]).unwrap();
    let plan = make_uninstall_plan(&manifest.id(), vec![]);
    db.apply_plan_transaction(&plan).unwrap();
    assert_eq!(
        db.entry_by_id(&pkg_id).unwrap().0,
        PackageState::PendingUninstall
    );
}

#[track_caller]
pub(crate) fn assert_plan_eq(actual: &ExecutionPlan, expected: &ExecutionPlan) {
    let actual_ops = actual.ops();
    let expected_ops = expected.ops();
    assert_eq!(
        expected_ops.len(),
        actual_ops.len(),
        "plan op count mismatch:\n  actual: {actual_ops:?}\nexpected: {expected_ops:?}",
    );
    for (actual_op, expected_op) in actual_ops.iter().zip(expected_ops) {
        match (actual_op, expected_op) {
            (
                ExecutionPlanOp::Install(InstallOp {
                    manifest: actual_manifest,
                    reason: actual_reason,
                    replacing_pkg_ids: actual_replacing_pkg_ids,
                }),
                ExecutionPlanOp::Install(InstallOp {
                    manifest: expected_manifest,
                    reason: expected_reason,
                    replacing_pkg_ids: expected_replacing_pkg_ids,
                }),
            ) => {
                assert!(Arc::ptr_eq(actual_manifest, expected_manifest));
                assert_eq!(actual_reason, expected_reason);
                assert_eq!(actual_replacing_pkg_ids, expected_replacing_pkg_ids);
            }
            (
                ExecutionPlanOp::Uninstall(UninstallOp {
                    pkg_id: actual_pkg_id,
                    reason: actual_reason,
                    conditions: actual_conditions,
                }),
                ExecutionPlanOp::Uninstall(UninstallOp {
                    pkg_id: expected_pkg_id,
                    reason: expected_reason,
                    conditions: expected_conditions,
                }),
            ) => {
                assert_eq!(actual_pkg_id, expected_pkg_id);
                assert_eq!(actual_reason, expected_reason);
                assert_eq!(actual_conditions, expected_conditions);
            }
            (
                ExecutionPlanOp::Skip(SkipOp {
                    pkg_spec: actual_pkg_spec,
                    reason: actual_reason,
                }),
                ExecutionPlanOp::Skip(SkipOp {
                    pkg_spec: expected_pkg_spec,
                    reason: expected_reason,
                }),
            ) => {
                assert_eq!(actual_pkg_spec, expected_pkg_spec);
                assert_eq!(actual_reason, expected_reason);
            }
            (actual_op, expected_op) => {
                panic!("mismatched plan ops\n  actual: {actual_op:?}\nexpected: {expected_op:?}")
            }
        }
    }
}

pub(crate) fn with_db<S, F, T>(cx: &ReportContext<S>, f: F) -> T
where
    S: ReportScope,
    F: FnOnce(PackageDatabase<'_>) -> T,
{
    let mut lock_file = engine::open_db_lock_file(cx).unwrap();
    let db = engine::load_database(cx, &mut lock_file).unwrap();
    f(db)
}
