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
    db::{PackageDatabase, PackageDatabaseError},
    engine::{
        ExecutionPlan, InstallOp, PlanStepOp, PreparedInstallStep, PreparedStep, PreparedStepOp,
        PreparedUninstallStep, StepId, StepResult, UninstallOp,
    },
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
    #[snafu(display("failed to apply execution plan transaction"))]
    ApplyTransaction { source: PackageDatabaseError },
}

impl From<PrepareErrorReport> for ReportValue<'static> {
    fn from(report: PrepareErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

#[derive(Debug)]
pub(crate) struct PreparedExecutions<'db, S>
where
    S: ReportScope,
{
    steps: Vec<PreparedStep<'db, S>>,
}

impl<'db, S> PreparedExecutions<'db, S>
where
    S: ReportScope,
{
    pub(in crate::engine) fn pop_executable(&mut self) -> Option<PreparedStep<'db, S>> {
        if let Some(idx) = self.steps.iter().position(PreparedStep::can_execute) {
            Some(self.steps.remove(idx))
        } else {
            None
        }
    }

    pub(in crate::engine) fn notify_result(&mut self, step_id: StepId, result: StepResult) {
        for exec in &mut self.steps {
            exec.notify_result(step_id, result);
        }
    }

    pub(in crate::engine) fn len(&self) -> usize {
        self.steps.len()
    }
}

pub(crate) fn prepare<'db, S>(
    cx: &ReportContext<S>,
    db: &Arc<Mutex<PackageDatabase<'db>>>,
    plan: &ExecutionPlan,
) -> Result<PreparedExecutions<'db, S>, S::Error>
where
    S: ReportScope,
{
    let outer_cx = cx;
    let cx = PrepareScope::start(cx);
    let mut executions = vec![];

    db.lock()
        .unwrap()
        .apply_plan_transaction(plan)
        .context(ApplyTransactionSnafu)
        .report_error(cx.reporter())?;

    for step in plan.steps() {
        let op: PreparedStepOp<'_, _> = match step.op() {
            PlanStepOp::Install(InstallOp {
                manifest,
                replacing_pkg_ids,
                ..
            }) => PreparedInstallStep::new(
                outer_cx,
                Arc::clone(db),
                Arc::clone(manifest),
                replacing_pkg_ids.clone(),
            )
            .into(),
            PlanStepOp::Uninstall(UninstallOp { pkg_id, .. }) => {
                PreparedUninstallStep::new(Arc::clone(db), pkg_id.clone()).into()
            }
            PlanStepOp::Skip(_) => continue,
        };
        executions.push(PreparedStep::with_conditions(
            step.step_id(),
            op,
            step.conditions().to_vec(),
        ));
    }

    Ok(PreparedExecutions { steps: executions })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cli::reporter::RootReportScope as _,
        engine::{
            InstallReason, PlanStep, PreparedStepOp, PreparedUninstallStep, SkipOp, SkipReason,
            StepCondition, StepId, StepResult, UninstallReason,
        },
        package::{InstallationState, PackageId},
        util::testing::{self, TempdirContext, TestScope},
    };

    #[test]
    fn prepare_applies_plan_transaction_to_unconditional_ops_and_skips_skip_ops() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let install_manifest = testing::make_manifest("install-font@0.1.0");
        let install_pkg_id = install_manifest.id();
        let uninstall_manifest = testing::make_manifest("uninstall-font@0.1.0");
        let uninstall_pkg_id = uninstall_manifest.id();
        let conditional_uninstall_manifest =
            testing::make_manifest("conditional-uninstall-font@0.1.0");
        let conditional_uninstall_pkg_id = conditional_uninstall_manifest.id();
        let skipped_pkg_id = "skipped-font@0.1.0".parse::<PackageId>().unwrap();

        testing::with_db(&cx, |mut db| {
            testing::mark_as_installed(&mut db, &uninstall_manifest);
            testing::mark_as_installed(&mut db, &conditional_uninstall_manifest);
            let db = Arc::new(Mutex::new(db));
            let install_step = PlanStep::new(InstallOp {
                manifest: Arc::clone(&install_manifest),
                reason: InstallReason::RequestedByUser,
                replacing_pkg_ids: vec![conditional_uninstall_pkg_id.clone()],
            });
            let install_step_id = install_step.step_id();
            let plan = ExecutionPlan::new_for_test([
                install_step,
                PlanStep::new(UninstallOp {
                    pkg_id: uninstall_pkg_id.clone(),
                    reason: UninstallReason::RequestedByUser,
                }),
                PlanStep::with_conditions(
                    UninstallOp {
                        pkg_id: conditional_uninstall_pkg_id.clone(),
                        reason: UninstallReason::ConflictWithInstall {
                            pkg_id: install_pkg_id.clone(),
                        },
                    },
                    vec![StepCondition::AfterSuccess(install_step_id)],
                ),
                PlanStep::new(SkipOp {
                    pkg_spec: skipped_pkg_id.clone().into(),
                    reason: SkipReason::AlreadyInstalled,
                }),
            ]);

            let prepared_plan = prepare(&cx, &db, &plan).unwrap();

            assert_eq!(prepared_plan.steps.len(), 3);
            assert!(
                prepared_plan
                    .steps
                    .iter()
                    .any(|execution| matches!(execution.op(), PreparedStepOp::Install(_)))
            );
            assert!(
                prepared_plan
                    .steps
                    .iter()
                    .any(|execution| matches!(execution.op(), PreparedStepOp::Uninstall(_)))
            );
            let db = db.lock().unwrap();
            assert_eq!(
                db.entry_by_id(&install_pkg_id).unwrap().installation_state,
                InstallationState::IncompleteInstall,
            );
            assert_eq!(
                db.entry_by_id(&uninstall_pkg_id)
                    .unwrap()
                    .installation_state,
                InstallationState::IncompleteUninstall,
            );
            assert_eq!(
                db.entry_by_id(&conditional_uninstall_pkg_id)
                    .unwrap()
                    .installation_state,
                InstallationState::Installed,
            );
            assert!(db.entry_by_id(&skipped_pkg_id).is_none());
        });
    }

    #[test]
    fn prepare_normalizes_incomplete_install_uninstall_target_to_incomplete_uninstall() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let manifest = testing::make_manifest("repair-font@0.1.0");
        let pkg_id = manifest.id();

        testing::with_db(&cx, |mut db| {
            testing::mark_as_incomplete_install(&mut db, &manifest);
            let db = Arc::new(Mutex::new(db));
            let plan = ExecutionPlan::new_for_test([PlanStep::new(UninstallOp {
                pkg_id: pkg_id.clone(),
                reason: UninstallReason::RequestedByUser,
            })]);

            let prepared_plan = prepare(&cx, &db, &plan).unwrap();

            assert_eq!(prepared_plan.steps.len(), 1);
            assert!(matches!(
                prepared_plan.steps[0].op(),
                PreparedStepOp::Uninstall(_)
            ));
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
    fn prepared_executions_unblocks_dependent_execution_after_success() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let dependency_pkg_id = "dependency-font@0.1.0".parse::<PackageId>().unwrap();
        let dependent_pkg_id = "dependent-font@0.1.0".parse::<PackageId>().unwrap();
        let dependency_step_id = StepId::new();
        let dependent_step_id = StepId::new();

        testing::with_db(&cx, |db| {
            let db = Arc::new(Mutex::new(db));
            let blocked_execution: PreparedStep<'_, TestScope> = PreparedStep::with_conditions(
                dependent_step_id,
                PreparedUninstallStep::new(Arc::clone(&db), dependent_pkg_id.clone()),
                vec![StepCondition::AfterSuccess(dependency_step_id)],
            );
            let ready_execution: PreparedStep<'_, TestScope> = PreparedStep::with_conditions(
                dependency_step_id,
                PreparedUninstallStep::new(Arc::clone(&db), dependency_pkg_id.clone()),
                vec![],
            );
            let mut prepared = PreparedExecutions {
                steps: vec![blocked_execution, ready_execution],
            };

            let first = prepared.pop_executable().unwrap();
            assert!(
                matches!(first.op(), PreparedStepOp::Uninstall(_) if first.step_id() == dependency_step_id)
            );

            prepared.notify_result(dependency_step_id, StepResult::Success);

            let second = prepared.pop_executable().unwrap();
            assert!(
                matches!(second.op(), PreparedStepOp::Uninstall(_) if second.step_id() == dependent_step_id)
            );
            assert!(prepared.pop_executable().is_none());
        });
    }

    #[test]
    fn prepared_executions_keep_dependent_execution_blocked_after_failure() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let dependency_pkg_id = "dependency-font@0.1.0".parse::<PackageId>().unwrap();
        let dependent_pkg_id = "dependent-font@0.1.0".parse::<PackageId>().unwrap();
        let dependency_step_id = StepId::new();
        let dependent_step_id = StepId::new();

        testing::with_db(&cx, |db| {
            let db = Arc::new(Mutex::new(db));
            let blocked_execution: PreparedStep<'_, TestScope> = PreparedStep::with_conditions(
                dependent_step_id,
                PreparedUninstallStep::new(Arc::clone(&db), dependent_pkg_id),
                vec![StepCondition::AfterSuccess(dependency_step_id)],
            );
            let ready_execution: PreparedStep<'_, TestScope> = PreparedStep::with_conditions(
                dependency_step_id,
                PreparedUninstallStep::new(Arc::clone(&db), dependency_pkg_id.clone()),
                vec![],
            );
            let mut prepared = PreparedExecutions {
                steps: vec![blocked_execution, ready_execution],
            };

            let first = prepared.pop_executable().unwrap();
            assert!(
                matches!(first.op(), PreparedStepOp::Uninstall(_) if first.step_id() == dependency_step_id)
            );

            prepared.notify_result(dependency_step_id, StepResult::Failure);

            assert!(prepared.pop_executable().is_none());
            assert_eq!(prepared.len(), 1);
        });
    }

    #[test]
    fn prepare_returns_no_executions_for_skip_only_plan() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let skipped_pkg_id = "skipped-font@0.1.0".parse::<PackageId>().unwrap();

        testing::with_db(&cx, |db| {
            let db = Arc::new(Mutex::new(db));
            let plan = ExecutionPlan::new_for_test([PlanStep::new(SkipOp {
                pkg_spec: skipped_pkg_id.clone().into(),
                reason: SkipReason::AlreadyInstalled,
            })]);

            let prepared_plan = prepare(&cx, &db, &plan).unwrap();

            assert!(prepared_plan.steps.is_empty());
            assert!(db.lock().unwrap().entry_by_id(&skipped_pkg_id).is_none());
        });
    }
}
