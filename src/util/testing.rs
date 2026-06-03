use std::{collections::HashMap, fmt::Debug, fs, ops::Deref, path::Path, sync::Arc};

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
        self, ExecutionPlan, InstallOp, InstallTargetSource, PlanStepOp, ResolvedInstallTarget,
        ResolvedUninstallTarget, SkipOp, StepCondition, StepId, UninstallOp,
    },
    package::{InstallationState, PackageDirs, PackageId, PackageManifest},
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
    ResolvedUninstallTarget::Uninstall {
        pkg_id: pkg_id.try_into().unwrap(),
    }
}

pub(crate) fn mark_as_state(
    db: &mut PackageDatabase<'_>,
    manifest: &Arc<PackageManifest>,
    state: InstallationState,
) {
    match state {
        InstallationState::Installed => mark_as_installed(db, manifest),
        InstallationState::IncompleteInstall => mark_as_incomplete_install(db, manifest),
        InstallationState::IncompleteUninstall => mark_as_incomplete_uninstall(db, manifest),
    }
}

pub(crate) fn mark_as_incomplete_install(
    db: &mut PackageDatabase<'_>,
    manifest: &Arc<PackageManifest>,
) {
    let pkg_id = manifest.id();
    let mut tx = db.transaction();
    tx.begin_install(Arc::clone(manifest));
    tx.commit().unwrap();
    assert_eq!(
        db.entry_by_id(&pkg_id).unwrap().installation_state,
        InstallationState::IncompleteInstall
    );
}

pub(crate) fn mark_as_installed(db: &mut PackageDatabase<'_>, manifest: &Arc<PackageManifest>) {
    let pkg_id = manifest.id();
    let mut tx = db.transaction();
    tx.begin_install(Arc::clone(manifest));
    tx.complete_install(&pkg_id, &[], &[]).unwrap();
    tx.commit().unwrap();
    assert_eq!(
        db.entry_by_id(&pkg_id).unwrap().installation_state,
        InstallationState::Installed
    );
}

pub(crate) fn mark_as_incomplete_uninstall(
    db: &mut PackageDatabase<'_>,
    manifest: &Arc<PackageManifest>,
) {
    let pkg_id = manifest.id();
    let mut tx = db.transaction();
    tx.begin_install(Arc::clone(manifest));
    tx.complete_install(&pkg_id, &[], &[]).unwrap();
    tx.begin_uninstall(&pkg_id).unwrap();
    tx.commit().unwrap();
    assert_eq!(
        db.entry_by_id(&pkg_id).unwrap().installation_state,
        InstallationState::IncompleteUninstall
    );
}

#[track_caller]
pub(crate) fn assert_plan_eq(actual: &ExecutionPlan, expected: &ExecutionPlan) {
    let actual_steps = actual.steps();
    let expected_steps = expected.steps();
    assert_eq!(
        expected_steps.len(),
        actual_steps.len(),
        "plan step count mismatch:\n  actual: {actual_steps:?}\nexpected: {expected_steps:?}",
    );
    let step_id_map = actual_steps
        .iter()
        .zip(expected_steps)
        .map(|(actual_step, expected_step)| (actual_step.step_id(), expected_step.step_id()))
        .collect::<HashMap<_, _>>();
    for (actual_step, expected_step) in actual_steps.iter().zip(expected_steps) {
        assert_conditions_eq(
            &step_id_map,
            actual_step.conditions(),
            expected_step.conditions(),
        );
        match (actual_step.op(), expected_step.op()) {
            (
                PlanStepOp::Install(InstallOp {
                    manifest: actual_manifest,
                    reason: actual_reason,
                    replacing_pkg_ids: actual_replacing_pkg_ids,
                }),
                PlanStepOp::Install(InstallOp {
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
                PlanStepOp::Uninstall(UninstallOp {
                    pkg_id: actual_pkg_id,
                    reason: actual_reason,
                }),
                PlanStepOp::Uninstall(UninstallOp {
                    pkg_id: expected_pkg_id,
                    reason: expected_reason,
                }),
            ) => {
                assert_eq!(actual_pkg_id, expected_pkg_id);
                assert_eq!(actual_reason, expected_reason);
            }
            (
                PlanStepOp::Skip(SkipOp {
                    pkg_spec: actual_pkg_spec,
                    reason: actual_reason,
                }),
                PlanStepOp::Skip(SkipOp {
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

fn assert_conditions_eq(
    step_id_map: &HashMap<StepId, StepId>,
    actual_conditions: &[StepCondition],
    expected_conditions: &[StepCondition],
) {
    let converted_actual_conditions = actual_conditions
        .iter()
        .map(|condition| match condition {
            StepCondition::AfterSuccess(step_id) => {
                StepCondition::AfterSuccess(step_id_map[step_id])
            }
            StepCondition::Never => StepCondition::Never,
        })
        .collect::<Vec<_>>();
    assert_eq!(converted_actual_conditions, expected_conditions);
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
