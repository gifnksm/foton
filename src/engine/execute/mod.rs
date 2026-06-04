use std::{
    collections::VecDeque,
    marker::PhantomData,
    sync::{Arc, Mutex},
};

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
    engine::{ExecutionPlan, PlanStep, StepId, prepare},
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
enum ExecuteErrorReport {
    #[snafu(display("failed to commit execution plan transaction"))]
    CommitTransaction { source: PackageDatabaseError },
}

impl From<ExecuteErrorReport> for ReportValue<'static> {
    fn from(report: ExecuteErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

#[derive(Debug)]
pub(in crate::engine) struct ExecutionState<'lock, S>
where
    S: ReportScope,
{
    ready: VecDeque<PreparedStep<'lock, S>>,
    pending: VecDeque<PlanStep>,
}

impl<'lock, S> ExecutionState<'lock, S>
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
        db: &Arc<Mutex<PackageDatabase<'lock>>>,
        tx: &mut PackageDatabaseTransaction<'_, 'lock>,
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
            let prepared = prepare::prepare_step(cx, db, tx, &step)?;
            if let Some(prepared) = prepared {
                self.ready.push_back(prepared);
                prepared_count += 1;
            }
        }
        Ok(prepared_count)
    }

    fn pop_ready(&mut self) -> Option<PreparedStep<'lock, S>> {
        self.ready.pop_front()
    }

    fn notify_result(&mut self, step_id: StepId, result: StepResult) {
        for pending in &mut self.pending {
            pending.notify_result(step_id, result);
        }
    }
}

#[derive(Debug, derive_more::From)]
pub(in crate::engine) struct PreparedStep<'db, S>
where
    S: ReportScope,
{
    step_id: StepId,
    op: PreparedStepOp<'db, S>,
}

#[derive(Debug, derive_more::From)]
pub(in crate::engine) enum PreparedStepOp<'db, S>
where
    S: ReportScope,
{
    Install(PreparedInstallStep<'db, S>),
    Uninstall(PreparedUninstallStep<'db>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::engine) enum StepResult {
    Success,
    Failure,
}

impl<'db, S> PreparedStep<'db, S>
where
    S: ReportScope,
{
    pub(in crate::engine) fn new<O>(step_id: StepId, op: O) -> Self
    where
        O: Into<PreparedStepOp<'db, S>>,
    {
        let op = op.into();
        Self { step_id, op }
    }

    #[cfg(test)]
    pub(in crate::engine) fn step_id(&self) -> StepId {
        self.step_id
    }

    #[cfg(test)]
    pub(in crate::engine) fn op(&self) -> &PreparedStepOp<'db, S> {
        &self.op
    }

    async fn execute(self, cx: &ReportContext<S>) -> Result<(), S::Error> {
        match self.op {
            PreparedStepOp::Install(exec) => exec.execute(cx).await,
            PreparedStepOp::Uninstall(exec) => exec.execute(cx),
        }
    }
}

pub(crate) async fn execute_plan<S>(
    cx: &ReportContext<S>,
    db: &Arc<Mutex<PackageDatabase<'_>>>,
    plan: ExecutionPlan,
) -> Result<(), S::Error>
where
    S: ReportScope,
{
    let cx = ExecuteScope::start(cx);
    let mut state = ExecutionState::new(plan.into_steps());
    {
        let mut db_guard = db.lock().unwrap();
        let mut tx = db_guard.transaction();
        let prepared_count = state.prepare_ready_steps(&cx, db, &mut tx)?;
        if prepared_count > 0 {
            tx.commit()
                .context(CommitTransactionSnafu)
                .report_error(cx.reporter())?;
        }
    }

    let mut err = None;
    while let Some(step) = state.pop_ready() {
        let step_id = step.step_id;
        if cx.cancel_token().is_cancelled() {
            return Err(S::Error::cancelled());
        }
        match step.execute(&cx).await {
            Ok(()) => state.notify_result(step_id, StepResult::Success),
            Err(e) => {
                state.notify_result(step_id, StepResult::Failure);
                err.get_or_insert(e);
            }
        }

        let mut db_guard = db.lock().unwrap();
        let mut tx = db_guard.transaction();
        let prepared_count = state.prepare_ready_steps(&cx, db, &mut tx)?;
        if prepared_count > 0 {
            tx.commit()
                .context(CommitTransactionSnafu)
                .report_error(cx.reporter())?;
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
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::{
        cli::reporter::RootReportScope as _,
        engine::{PlanStep, PreparedStepOp, StepCondition, UninstallOp, UninstallReason},
        util::testing::{self, TempdirContext, TestScope},
    };

    fn prepare_ready_steps<'db>(
        cx: &ReportContext<TestScope>,
        db: &Arc<Mutex<PackageDatabase<'db>>>,
        state: &mut ExecutionState<'db, TestScope>,
    ) {
        let mut db_guard = db.lock().unwrap();
        let mut tx = db_guard.transaction();
        let prepared_count = state.prepare_ready_steps(cx, db, &mut tx).unwrap();
        if prepared_count > 0 {
            tx.commit().unwrap();
        }
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
