use std::{collections::VecDeque, marker::PhantomData};

use snafu::{ResultExt as _, Snafu};

use crate::{
    cli::{
        context::ReportContext,
        reporter::{
            NeverReport, OperationError as _, ReportScope, ReportValue, ScopeResultErrorExt as _,
            SubReportScope,
        },
    },
    db::{PackageDatabase, PackageDatabaseError, PackageDatabaseTransaction},
    engine::{ExecutionPlan, PlanStep, PlanStepOp, StepId, StepResult},
    package::PackageId,
    util::macros::concat_line,
};

pub(in crate::engine) use self::{install::*, uninstall::*};

mod install;
mod uninstall;

#[derive(Debug)]
struct ExecuteScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for ExecuteScope<S>
where
    S: ReportScope,
{
    type NoticeReportValue = NeverReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = ExecuteErrorReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for ExecuteScope<S>
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
#[expect(clippy::enum_variant_names)]
enum ExecuteErrorReport {
    #[snafu(display("failed to commit execution plan transaction"))]
    CommitTransaction { source: PackageDatabaseError },
    #[snafu(display(
        concat_line!(
            "failed to commit database changes after attempting to install package {pkg_id}",
            "run `foton repair {pkg_id}` to restore a consistent state",
            "if repair does not resolve the problem, manual cleanup may be required",
        ),
        pkg_id = pkg_id,
    ))]
    CommitInstallTransaction {
        pkg_id: PackageId,
        source: PackageDatabaseError,
    },
    #[snafu(display(
        concat_line!(
            "failed to commit database changes after attempting to uninstall package {pkg_id}",
            "font unregistration and package file removal may already have been applied",
            "run `foton repair {pkg_id}` to restore a consistent state",
            "if repair does not resolve the problem, manual database cleanup may be required",
        ),
        pkg_id = pkg_id,
    ))]
    CommitUninstallTransaction {
        pkg_id: PackageId,
        source: PackageDatabaseError,
    },
}

impl From<ExecuteErrorReport> for ReportValue<'static> {
    fn from(report: ExecuteErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

#[derive(Debug)]
pub(in crate::engine) struct ExecutionState<S>
where
    S: ReportScope,
{
    ready: VecDeque<PreparedStep<S>>,
    pending: VecDeque<PlanStep>,
}

impl<S> ExecutionState<S>
where
    S: ReportScope,
{
    pub(in crate::engine) fn new<I>(plan: I) -> Self
    where
        I: IntoIterator<Item = PlanStep>,
    {
        Self {
            ready: VecDeque::new(),
            pending: plan.into_iter().collect(),
        }
    }

    fn pending_len(&self) -> usize {
        self.pending.len()
    }

    fn prepare_ready_steps(
        &mut self,
        cx: &ReportContext<S>,
        tx: &mut PackageDatabaseTransaction<'_, '_>,
    ) -> Result<usize, S::Error>
    where
        S: ReportScope,
    {
        let count = self.pending.len();
        let mut prepared_count = 0;
        for _ in 0..count {
            let step = self.pending.pop_front().unwrap();
            if !step.can_execute() {
                self.pending.push_back(step);
                continue;
            }
            let prepared = PreparedStep::from_plan_step(cx, tx, step)?;
            if let Some(prepared) = prepared {
                self.ready.push_back(prepared);
                prepared_count += 1;
            }
        }
        Ok(prepared_count)
    }

    fn pop_ready(&mut self) -> Option<PreparedStep<S>> {
        self.ready.pop_front()
    }

    fn notify_result(&mut self, step_id: StepId, result: StepResult) {
        for pending in &mut self.pending {
            pending.notify_result(step_id, result);
        }
    }
}

#[derive(Debug, derive_more::From)]
pub(in crate::engine) struct PreparedStep<S>
where
    S: ReportScope,
{
    step_id: StepId,
    op: PreparedStepOp<S>,
}

#[derive(Debug, derive_more::From)]
pub(in crate::engine) enum PreparedStepOp<S>
where
    S: ReportScope,
{
    Install(Box<PreparedInstallStep<S>>),
    Uninstall(PreparedUninstallStep),
}

impl<S> PreparedStep<S>
where
    S: ReportScope,
{
    pub(in crate::engine) fn from_plan_step(
        cx: &ReportContext<S>,
        tx: &mut PackageDatabaseTransaction<'_, '_>,
        step: PlanStep,
    ) -> Result<Option<Self>, S::Error> {
        assert!(step.can_execute());
        let step_id = step.step_id();
        let op = match step.into_op() {
            PlanStepOp::Install(op) => Box::new(PreparedInstallStep::from_plan_step(tx, op)).into(),
            PlanStepOp::Uninstall(op) => PreparedUninstallStep::from_plan_step(cx, tx, op)?.into(),
            PlanStepOp::Skip(_) => return Ok(None),
        };
        Ok(Some(Self { step_id, op }))
    }

    #[cfg(test)]
    pub(in crate::engine) fn step_id(&self) -> StepId {
        self.step_id
    }

    #[cfg(test)]
    pub(in crate::engine) fn op(&self) -> &PreparedStepOp<S> {
        &self.op
    }

    async fn execute(&mut self, cx: &ReportContext<S>) -> Result<(), S::Error> {
        match &mut self.op {
            PreparedStepOp::Install(op) => op.execute(cx).await,
            PreparedStepOp::Uninstall(op) => op.execute(cx),
        }
    }

    fn on_complete(
        &mut self,
        cx: &ReportContext<S>,
        tx: &mut PackageDatabaseTransaction<'_, '_>,
    ) -> Result<(), S::Error> {
        match &mut self.op {
            PreparedStepOp::Install(op) => op.on_complete(cx, tx),
            PreparedStepOp::Uninstall(op) => op.on_complete(cx, tx),
        }
    }

    fn after_commit(&mut self) {
        match &mut self.op {
            PreparedStepOp::Install(op) => op.after_commit(),
            PreparedStepOp::Uninstall(op) => op.after_commit(),
        }
    }

    fn on_failure(&mut self, cx: &ReportContext<S>, tx: &mut PackageDatabaseTransaction<'_, '_>) {
        match &mut self.op {
            PreparedStepOp::Install(op) => op.on_failure(cx, tx),
            PreparedStepOp::Uninstall(op) => op.on_failure(cx, tx),
        }
    }

    fn commit_error_report(&self, source: PackageDatabaseError) -> ExecuteErrorReport {
        match &self.op {
            PreparedStepOp::Install(op) => ExecuteErrorReport::CommitInstallTransaction {
                pkg_id: op.pkg_id(),
                source,
            },
            PreparedStepOp::Uninstall(op) => ExecuteErrorReport::CommitUninstallTransaction {
                pkg_id: op.pkg_id().clone(),
                source,
            },
        }
    }
}

pub(crate) async fn execute_plan<S>(
    cx: &ReportContext<S>,
    db: &mut PackageDatabase<'_>,
    plan: ExecutionPlan,
) -> Result<(), S::Error>
where
    S: ReportScope,
{
    let cx = ExecuteScope::start(cx);
    let mut state = ExecutionState::new(plan.into_steps());
    {
        let mut tx = db.transaction();
        let prepared_count = state.prepare_ready_steps(&cx, &mut tx)?;
        if prepared_count > 0 {
            tx.commit()
                .context(CommitTransactionSnafu)
                .report_error(cx.reporter())?;
        }
    }

    let mut err = None;
    while let Some(mut step) = state.pop_ready() {
        let step_id = step.step_id;
        if cx.cancel_token().is_cancelled() {
            return Err(S::Error::cancelled());
        }

        let mut res = step.execute(&cx).await;
        let mut tx = db.transaction();
        if res.is_ok()
            && let Err(err) = step.on_complete(&cx, &mut tx)
        {
            res = Err(err);
        }
        if res.is_err() {
            step.on_failure(&cx, &mut tx);
        }
        let res = match res {
            Ok(()) => StepResult::Success,
            Err(e) => {
                err.get_or_insert(e);
                StepResult::Failure
            }
        };
        state.notify_result(step_id, res);
        state.prepare_ready_steps(&cx, &mut tx)?;
        let commit_result = tx.commit();
        if let Err(source) = commit_result {
            return Err(cx.reporter().report_error(step.commit_error_report(source)));
        }
        if res.is_success() {
            step.after_commit();
        }
    }
    let len = state.pending_len();
    if len > 0 {
        cx.reporter().report_info(format_args!(
            "{len} step(s) were not executed due to unsatisfied conditions."
        ));
    }

    if let Some(err) = err {
        return Err(err);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        str::FromStr as _,
        sync::{Arc, Mutex},
    };

    use super::*;
    use crate::{
        cli::reporter::RootReportScope as _,
        engine::{
            InstallOp, InstallReason, PlanStep, PreparedStepOp, SkipOp, SkipReason, StepCondition,
            UninstallOp, UninstallReason,
        },
        package::{InstallationState, PackageId},
        util::testing::{self, TempdirContext, TestScope},
    };

    fn prepare_ready_steps(
        cx: &ReportContext<TestScope>,
        db: &Arc<Mutex<PackageDatabase<'_>>>,
        state: &mut ExecutionState<TestScope>,
    ) {
        let mut db_guard = db.lock().unwrap();
        let mut tx = db_guard.transaction();
        let prepared_count = state.prepare_ready_steps(cx, &mut tx).unwrap();
        if prepared_count > 0 {
            tx.commit().unwrap();
        }
    }

    #[test]
    fn prepared_step_from_plan_step_begins_install_for_unconditional_install_step() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let manifest = testing::make_manifest("install-font@0.1.0");
        let pkg_id = manifest.id();

        testing::with_db(&cx, |mut db| {
            let step = PlanStep::new(InstallOp {
                manifest: Arc::clone(&manifest),
                reason: InstallReason::RequestedByUser,
            });

            let mut tx = db.transaction();
            let prepared = PreparedStep::from_plan_step(&cx, &mut tx, step)
                .unwrap()
                .unwrap();
            tx.commit().unwrap();

            assert!(matches!(prepared.op(), PreparedStepOp::Install(_)));
            assert_eq!(
                db.entry_by_id(&pkg_id).unwrap().installation_state,
                InstallationState::IncompleteInstall,
            );
        });
    }

    #[test]
    fn prepared_step_from_plan_step_begins_uninstall_for_unconditional_uninstall_step() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let manifest = testing::make_manifest("uninstall-font@0.1.0");
        let pkg_id = manifest.id();

        testing::with_db(&cx, |mut db| {
            testing::mark_as_installed(&mut db, &manifest);
            let step = PlanStep::new(UninstallOp {
                pkg_id: pkg_id.clone(),
                reason: UninstallReason::RequestedByUser,
            });

            let mut tx = db.transaction();
            let prepared = PreparedStep::from_plan_step(&cx, &mut tx, step)
                .unwrap()
                .unwrap();
            tx.commit().unwrap();

            assert!(matches!(prepared.op(), PreparedStepOp::Uninstall(_)));
            assert_eq!(
                db.entry_by_id(&pkg_id).unwrap().installation_state,
                InstallationState::IncompleteUninstall,
            );
        });
    }

    #[test]
    fn prepared_step_from_plan_step_returns_none_for_skip_ops() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let skipped_pkg_id = PackageId::from_str("skipped-font@0.1.0").unwrap();

        testing::with_db(&cx, |mut db| {
            let step = PlanStep::new(SkipOp {
                pkg_spec: skipped_pkg_id.clone().into(),
                reason: SkipReason::AlreadyInstalled,
            });

            let mut tx = db.transaction();
            let prepared = PreparedStep::from_plan_step(&cx, &mut tx, step).unwrap();

            assert!(prepared.is_none());
            assert!(db.entry_by_id(&skipped_pkg_id).is_none());
        });
    }

    #[test]
    fn execution_state_does_not_begin_conditional_uninstall_before_dependency_success() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let dependency_manifest = testing::make_manifest("dependency-font@0.1.0");
        let dependency_pkg_id = dependency_manifest.id();
        let dependent_manifest = testing::make_manifest("dependent-font@0.1.0");
        let dependent_pkg_id = dependent_manifest.id();

        testing::with_db(&cx, |mut db| {
            testing::mark_as_installed(&mut db, &dependency_manifest);
            testing::mark_as_installed(&mut db, &dependent_manifest);
            let db = Arc::new(Mutex::new(db));
            let dependency_step = PlanStep::new(UninstallOp {
                pkg_id: dependency_pkg_id.clone(),
                reason: UninstallReason::RequestedByUser,
            });
            let dependency_step_id = dependency_step.step_id();
            let dependent_step = PlanStep::with_conditions(
                UninstallOp {
                    pkg_id: dependent_pkg_id.clone(),
                    reason: UninstallReason::RequestedByUser,
                },
                vec![StepCondition::AfterSuccess(dependency_step_id)],
            );
            let mut state = ExecutionState::new([dependent_step, dependency_step]);

            prepare_ready_steps(&cx, &db, &mut state);

            assert_eq!(
                db.lock()
                    .unwrap()
                    .entry_by_id(&dependency_pkg_id)
                    .unwrap()
                    .installation_state,
                InstallationState::IncompleteUninstall,
            );
            assert_eq!(
                db.lock()
                    .unwrap()
                    .entry_by_id(&dependent_pkg_id)
                    .unwrap()
                    .installation_state,
                InstallationState::Installed,
            );
        });
    }

    #[test]
    fn execution_state_unblocks_dependent_step_after_success() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let dependency_manifest = testing::make_manifest("dependency-font@0.1.0");
        let dependency_pkg_id = dependency_manifest.id();
        let dependent_manifest = testing::make_manifest("dependent-font@0.1.0");
        let dependent_pkg_id = dependent_manifest.id();

        testing::with_db(&cx, |mut db| {
            testing::mark_as_installed(&mut db, &dependency_manifest);
            testing::mark_as_installed(&mut db, &dependent_manifest);
            let db = Arc::new(Mutex::new(db));
            let dependency_step = PlanStep::new(UninstallOp {
                pkg_id: dependency_pkg_id.clone(),
                reason: UninstallReason::RequestedByUser,
            });
            let dependency_step_id = dependency_step.step_id();
            let dependent_step = PlanStep::with_conditions(
                UninstallOp {
                    pkg_id: dependent_pkg_id.clone(),
                    reason: UninstallReason::RequestedByUser,
                },
                vec![StepCondition::AfterSuccess(dependency_step_id)],
            );
            let mut state = ExecutionState::new([dependent_step, dependency_step]);

            prepare_ready_steps(&cx, &db, &mut state);

            let first = state.pop_ready().unwrap();
            assert!(matches!(
                first.op(),
                PreparedStepOp::Uninstall(_) if first.step_id() == dependency_step_id
            ));

            state.notify_result(dependency_step_id, StepResult::Success);
            prepare_ready_steps(&cx, &db, &mut state);

            let second = state.pop_ready().unwrap();
            assert!(matches!(
                second.op(),
                PreparedStepOp::Uninstall(_) if second.step_id() != dependency_step_id
            ));
            assert!(state.pop_ready().is_none());
        });
    }

    #[test]
    fn execution_state_keeps_dependent_step_pending_after_failure() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let dependency_manifest = testing::make_manifest("dependency-font@0.1.0");
        let dependency_pkg_id = dependency_manifest.id();
        let dependent_manifest = testing::make_manifest("dependent-font@0.1.0");
        let dependent_pkg_id = dependent_manifest.id();

        testing::with_db(&cx, |mut db| {
            testing::mark_as_installed(&mut db, &dependency_manifest);
            testing::mark_as_installed(&mut db, &dependent_manifest);
            let db = Arc::new(Mutex::new(db));
            let dependency_step = PlanStep::new(UninstallOp {
                pkg_id: dependency_pkg_id.clone(),
                reason: UninstallReason::RequestedByUser,
            });
            let dependency_step_id = dependency_step.step_id();
            let dependent_step = PlanStep::with_conditions(
                UninstallOp {
                    pkg_id: dependent_pkg_id,
                    reason: UninstallReason::RequestedByUser,
                },
                vec![StepCondition::AfterSuccess(dependency_step_id)],
            );
            let mut state = ExecutionState::new([dependent_step, dependency_step]);

            prepare_ready_steps(&cx, &db, &mut state);

            let first = state.pop_ready().unwrap();
            assert!(matches!(
                first.op(),
                PreparedStepOp::Uninstall(_) if first.step_id() == dependency_step_id
            ));

            state.notify_result(dependency_step_id, StepResult::Failure);
            prepare_ready_steps(&cx, &db, &mut state);

            assert!(state.pop_ready().is_none());
            assert_eq!(state.pending_len(), 1);
        });
    }
}
