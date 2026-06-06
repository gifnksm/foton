use std::{collections::VecDeque, fmt::Debug, marker::PhantomData};

use async_trait::async_trait;
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
    op: Box<dyn PreparedStepOp<S>>,
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
            PlanStepOp::Install(op) => {
                Box::new(PreparedInstallStep::from_plan_step(tx, op)) as Box<dyn PreparedStepOp<S>>
            }
            PlanStepOp::Uninstall(op) => {
                Box::new(PreparedUninstallStep::from_plan_step(cx, tx, op)?)
            }
            PlanStepOp::Skip(_) => return Ok(None),
        };
        Ok(Some(Self { step_id, op }))
    }

    #[cfg(test)]
    pub(in crate::engine) fn step_id(&self) -> StepId {
        self.step_id
    }

    async fn execute(&mut self, cx: &ReportContext<S>) -> Result<(), S::Error> {
        self.op.execute(cx).await
    }

    fn on_complete(
        &mut self,
        cx: &ReportContext<S>,
        tx: &mut PackageDatabaseTransaction<'_, '_>,
    ) -> Result<(), S::Error> {
        self.op.on_complete(cx, tx)
    }

    fn after_commit(&mut self) {
        self.op.after_commit();
    }

    fn on_failure(&mut self, cx: &ReportContext<S>, tx: &mut PackageDatabaseTransaction<'_, '_>) {
        self.op.on_failure(cx, tx);
    }

    fn commit_error_report(&self, source: PackageDatabaseError) -> ExecuteErrorReport {
        self.op.commit_error_report(source)
    }
}

#[async_trait]
trait PreparedStepOp<S>: Debug + Send + Sync
where
    S: ReportScope,
{
    async fn execute(&mut self, cx: &ReportContext<S>) -> Result<(), S::Error>;
    fn on_complete(
        &mut self,
        cx: &ReportContext<S>,
        tx: &mut PackageDatabaseTransaction<'_, '_>,
    ) -> Result<(), S::Error>;
    fn after_commit(&mut self);
    fn on_failure(&mut self, cx: &ReportContext<S>, tx: &mut PackageDatabaseTransaction<'_, '_>);
    fn commit_error_report(&self, source: PackageDatabaseError) -> ExecuteErrorReport;
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
    use std::{str::FromStr as _, sync::Arc};

    use super::*;
    use crate::{
        cli::reporter::RootReportScope as _,
        engine::{
            InstallOp, InstallReason, PlanStep, SkipOp, SkipReason, StepCondition, UninstallOp,
            UninstallReason,
        },
        package::PackageId,
        util::testing::{self, TempdirContext, TestScope},
    };

    fn prepare_ready_steps(
        cx: &ReportContext<TestScope>,
        db: &mut PackageDatabase<'_>,
        state: &mut ExecutionState<TestScope>,
    ) {
        let mut tx = db.transaction();
        let prepared_count = state.prepare_ready_steps(cx, &mut tx).unwrap();
        if prepared_count > 0 {
            tx.commit().unwrap();
        }
    }

    fn conditional_uninstall_state(
        db: &mut PackageDatabase<'_>,
    ) -> (ExecutionState<TestScope>, StepId, StepId) {
        let dependency_manifest = testing::make_manifest("dependency-font@0.1.0");
        let dependency_pkg_id = dependency_manifest.id();
        let dependent_manifest = testing::make_manifest("dependent-font@0.1.0");
        let dependent_pkg_id = dependent_manifest.id();

        testing::mark_as_installed(db, &dependency_manifest);
        testing::mark_as_installed(db, &dependent_manifest);

        let dependency_step = PlanStep::new(UninstallOp {
            pkg_id: dependency_pkg_id,
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
        let dependent_step_id = dependent_step.step_id();

        (
            ExecutionState::new([dependent_step, dependency_step]),
            dependency_step_id,
            dependent_step_id,
        )
    }

    mod prepared_step_from_plan_step {
        use super::*;

        #[test]
        fn returns_prepared_step_for_install_ops() {
            let cx = TempdirContext::new();
            let cx = TestScope::start(&cx);
            let manifest = testing::make_manifest("install-font@0.1.0");

            testing::with_db(&cx, |mut db| {
                let step = PlanStep::new(InstallOp {
                    manifest: Arc::clone(&manifest),
                    reason: InstallReason::RequestedByUser,
                });
                let step_id = step.step_id();

                let mut tx = db.transaction();
                let prepared = PreparedStep::from_plan_step(&cx, &mut tx, step)
                    .unwrap()
                    .unwrap();

                assert_eq!(prepared.step_id(), step_id);
            });
        }

        #[test]
        fn returns_prepared_step_for_uninstall_ops() {
            let cx = TempdirContext::new();
            let cx = TestScope::start(&cx);
            let manifest = testing::make_manifest("uninstall-font@0.1.0");
            let pkg_id = manifest.id();

            testing::with_db(&cx, |mut db| {
                testing::mark_as_installed(&mut db, &manifest);
                let step = PlanStep::new(UninstallOp {
                    pkg_id,
                    reason: UninstallReason::RequestedByUser,
                });
                let step_id = step.step_id();

                let mut tx = db.transaction();
                let prepared = PreparedStep::from_plan_step(&cx, &mut tx, step)
                    .unwrap()
                    .unwrap();

                assert_eq!(prepared.step_id(), step_id);
            });
        }

        #[test]
        fn returns_none_for_skip_ops() {
            let cx = TempdirContext::new();
            let cx = TestScope::start(&cx);
            let skipped_pkg_id = PackageId::from_str("skipped-font@0.1.0").unwrap();

            testing::with_db(&cx, |mut db| {
                let step = PlanStep::new(SkipOp {
                    pkg_spec: skipped_pkg_id.into(),
                    reason: SkipReason::AlreadyInstalled,
                });

                let mut tx = db.transaction();
                let prepared = PreparedStep::from_plan_step(&cx, &mut tx, step).unwrap();

                assert!(prepared.is_none());
            });
        }
    }

    mod execution_state {
        use super::*;

        #[test]
        fn prepare_ready_steps_only_enqueues_executable_steps() {
            let cx = TempdirContext::new();
            let cx = TestScope::start(&cx);

            testing::with_db(&cx, |mut db| {
                let (mut state, dependency_step_id, _dependent_step_id) =
                    conditional_uninstall_state(&mut db);

                prepare_ready_steps(&cx, &mut db, &mut state);

                let first = state.pop_ready().unwrap();
                assert_eq!(first.step_id(), dependency_step_id);
                assert!(state.pop_ready().is_none());
                assert_eq!(state.pending_len(), 1);
            });
        }

        #[test]
        fn notify_result_success_unblocks_dependent_step() {
            let cx = TempdirContext::new();
            let cx = TestScope::start(&cx);

            testing::with_db(&cx, |mut db| {
                let (mut state, dependency_step_id, dependent_step_id) =
                    conditional_uninstall_state(&mut db);

                prepare_ready_steps(&cx, &mut db, &mut state);

                let first = state.pop_ready().unwrap();
                assert_eq!(first.step_id(), dependency_step_id);

                state.notify_result(dependency_step_id, StepResult::Success);
                prepare_ready_steps(&cx, &mut db, &mut state);

                let second = state.pop_ready().unwrap();
                assert_eq!(second.step_id(), dependent_step_id);
                assert!(state.pop_ready().is_none());
                assert_eq!(state.pending_len(), 0);
            });
        }

        #[test]
        fn notify_result_failure_keeps_dependent_step_pending() {
            let cx = TempdirContext::new();
            let cx = TestScope::start(&cx);

            testing::with_db(&cx, |mut db| {
                let (mut state, dependency_step_id, _dependent_step_id) =
                    conditional_uninstall_state(&mut db);

                prepare_ready_steps(&cx, &mut db, &mut state);

                let first = state.pop_ready().unwrap();
                assert_eq!(first.step_id(), dependency_step_id);

                state.notify_result(dependency_step_id, StepResult::Failure);
                prepare_ready_steps(&cx, &mut db, &mut state);

                assert!(state.pop_ready().is_none());
                assert_eq!(state.pending_len(), 1);
            });
        }
    }
}
