use std::{
    marker::PhantomData,
    sync::{Arc, Mutex},
};

use snafu::{ResultExt as _, Snafu};

use crate::{
    cli::{
        context::ReportContext,
        reporter::{
            NeverReport, ReportScope, ReportValue, ScopeResultErrorExt as _, SubReportScope,
        },
    },
    db::{PackageDatabase, PackageDatabaseError, PackageDatabaseTransaction},
    engine::{
        InstallOp, PlanStep, PlanStepOp, PreparedInstallStep, PreparedStep, PreparedStepOp,
        PreparedUninstallStep, UninstallOp,
    },
    package::PackageId,
};

#[derive(Debug)]
struct PrepareScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for PrepareScope<S>
where
    S: ReportScope,
{
    type NoticeReportValue = NeverReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = PrepareErrorReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for PrepareScope<S>
where
    S: ReportScope,
{
    fn new() -> Self {
        Self {
            _base_scope: PhantomData,
        }
    }
}

#[derive(Debug, Snafu)]
enum PrepareErrorReport {
    #[snafu(display("failed to start uninstall transaction for package {pkg_id}"))]
    BeginUninstall {
        pkg_id: PackageId,
        source: PackageDatabaseError,
    },
}

impl From<PrepareErrorReport> for ReportValue<'static> {
    fn from(report: PrepareErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

pub(in crate::engine) fn prepare_step<'lock, S>(
    cx: &ReportContext<S>,
    db: &Arc<Mutex<PackageDatabase<'lock>>>,
    tx: &mut PackageDatabaseTransaction<'_, '_>,
    step: &PlanStep,
) -> Result<Option<PreparedStep<'lock, S>>, S::Error>
where
    S: ReportScope,
{
    let outer_cx = cx;
    let cx = PrepareScope::start(cx);

    let op: PreparedStepOp<'_, _> = match step.op() {
        PlanStepOp::Install(InstallOp {
            manifest,
            replacing_pkg_ids,
            ..
        }) => {
            if step.can_execute() {
                tx.begin_install(Arc::clone(manifest));
            }
            PreparedInstallStep::new(
                outer_cx,
                Arc::clone(db),
                Arc::clone(manifest),
                replacing_pkg_ids.clone(),
            )
            .into()
        }
        PlanStepOp::Uninstall(UninstallOp { pkg_id, .. }) => {
            if step.can_execute() {
                tx.begin_uninstall(pkg_id)
                    .context(BeginUninstallSnafu { pkg_id })
                    .report_error(cx.reporter())?;
            }
            PreparedUninstallStep::new(Arc::clone(db), pkg_id.clone()).into()
        }
        PlanStepOp::Skip(_) => return Ok(None),
    };
    Ok(Some(PreparedStep::new(step.step_id(), op)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cli::reporter::RootReportScope as _,
        engine::{
            InstallReason, PlanStep, PreparedStepOp, SkipOp, SkipReason, StepCondition, StepId,
            UninstallReason,
        },
        package::{InstallationState, PackageId},
        util::testing::{self, TempdirContext, TestScope},
    };

    fn prepare_step_and_commit<'db>(
        cx: &ReportContext<TestScope>,
        db: &Arc<Mutex<PackageDatabase<'db>>>,
        step: &PlanStep,
    ) -> Option<PreparedStep<'db, TestScope>> {
        let mut db_guard = db.lock().unwrap();
        let mut tx = db_guard.transaction();
        let prepared = prepare_step(cx, db, &mut tx, step).unwrap();
        tx.commit().unwrap();
        prepared
    }

    #[test]
    fn prepare_step_begins_install_for_unconditional_install_step() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let manifest = testing::make_manifest("install-font@0.1.0");
        let pkg_id = manifest.id();

        testing::with_db(&cx, |db| {
            let db = Arc::new(Mutex::new(db));
            let step = PlanStep::new(InstallOp {
                manifest: Arc::clone(&manifest),
                reason: InstallReason::RequestedByUser,
                replacing_pkg_ids: vec![],
            });

            let prepared = prepare_step_and_commit(&cx, &db, &step).unwrap();

            assert!(matches!(prepared.op(), PreparedStepOp::Install(_)));
            assert_eq!(
                db.lock()
                    .unwrap()
                    .entry_by_id(&pkg_id)
                    .unwrap()
                    .installation_state,
                InstallationState::IncompleteInstall,
            );
        });
    }

    #[test]
    fn prepare_step_begins_uninstall_for_unconditional_uninstall_step() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let manifest = testing::make_manifest("uninstall-font@0.1.0");
        let pkg_id = manifest.id();

        testing::with_db(&cx, |mut db| {
            testing::mark_as_installed(&mut db, &manifest);
            let db = Arc::new(Mutex::new(db));
            let step = PlanStep::new(UninstallOp {
                pkg_id: pkg_id.clone(),
                reason: UninstallReason::RequestedByUser,
            });

            let prepared = prepare_step_and_commit(&cx, &db, &step).unwrap();

            assert!(matches!(prepared.op(), PreparedStepOp::Uninstall(_)));
            assert_eq!(
                db.lock()
                    .unwrap()
                    .entry_by_id(&pkg_id)
                    .unwrap()
                    .installation_state,
                InstallationState::IncompleteUninstall,
            );
        });
    }

    #[test]
    fn prepare_step_does_not_begin_conditional_uninstall_step() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let manifest = testing::make_manifest("conditional-uninstall-font@0.1.0");
        let pkg_id = manifest.id();
        let dependency_step_id = StepId::new();

        testing::with_db(&cx, |mut db| {
            testing::mark_as_installed(&mut db, &manifest);
            let db = Arc::new(Mutex::new(db));
            let step = PlanStep::with_conditions(
                UninstallOp {
                    pkg_id: pkg_id.clone(),
                    reason: UninstallReason::RequestedByUser,
                },
                vec![StepCondition::AfterSuccess(dependency_step_id)],
            );

            let prepared = prepare_step_and_commit(&cx, &db, &step).unwrap();

            assert!(matches!(prepared.op(), PreparedStepOp::Uninstall(_)));
            assert_eq!(
                db.lock()
                    .unwrap()
                    .entry_by_id(&pkg_id)
                    .unwrap()
                    .installation_state,
                InstallationState::Installed,
            );
        });
    }

    #[test]
    fn prepare_step_normalizes_incomplete_install_target_to_incomplete_uninstall() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let manifest = testing::make_manifest("repair-font@0.1.0");
        let pkg_id = manifest.id();

        testing::with_db(&cx, |mut db| {
            testing::mark_as_incomplete_install(&mut db, &manifest);
            let db = Arc::new(Mutex::new(db));
            let step = PlanStep::new(UninstallOp {
                pkg_id: pkg_id.clone(),
                reason: UninstallReason::RequestedByUser,
            });

            let prepared = prepare_step_and_commit(&cx, &db, &step).unwrap();

            assert!(matches!(prepared.op(), PreparedStepOp::Uninstall(_)));
            assert_eq!(
                db.lock()
                    .unwrap()
                    .entry_by_id(&pkg_id)
                    .unwrap()
                    .installation_state,
                InstallationState::IncompleteUninstall,
            );
        });
    }

    #[test]
    fn prepare_step_returns_none_for_skip_ops() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let skipped_pkg_id = "skipped-font@0.1.0".parse::<PackageId>().unwrap();

        testing::with_db(&cx, |db| {
            let db = Arc::new(Mutex::new(db));
            let step = PlanStep::new(SkipOp {
                pkg_spec: skipped_pkg_id.clone().into(),
                reason: SkipReason::AlreadyInstalled,
            });

            let mut db_guard = db.lock().unwrap();
            let mut tx = db_guard.transaction();
            let prepared = prepare_step(&cx, &db, &mut tx, &step).unwrap();
            drop(tx);
            drop(db_guard);

            assert!(prepared.is_none());
            assert!(db.lock().unwrap().entry_by_id(&skipped_pkg_id).is_none());
        });
    }
}
