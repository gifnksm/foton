use crate::{
    cli::{
        context::ReportContext,
        reporter::{OperationError as _, ReportScope},
    },
    engine::{PreparedExecutions, StepCondition, StepId},
};

pub(in crate::engine) use self::{install::*, uninstall::*};

mod install;
mod uninstall;

#[derive(Debug, derive_more::From)]
pub(in crate::engine) struct PreparedStep<'db, S>
where
    S: ReportScope,
{
    step_id: StepId,
    op: PreparedStepOp<'db, S>,
    conditions: Vec<StepCondition>,
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
    pub(in crate::engine) fn with_conditions<O>(
        step_id: StepId,
        op: O,
        conditions: Vec<StepCondition>,
    ) -> Self
    where
        O: Into<PreparedStepOp<'db, S>>,
    {
        let op = op.into();
        Self {
            step_id,
            op,
            conditions,
        }
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

    pub(in crate::engine) fn can_execute(&self) -> bool {
        self.conditions.is_empty()
    }

    pub(in crate::engine) fn notify_result(&mut self, step_id: StepId, result: StepResult) {
        self.conditions.retain_mut(|exec| match exec {
            StepCondition::AfterSuccess(cond_id) if *cond_id == step_id => match result {
                StepResult::Success => false,
                StepResult::Failure => {
                    *exec = StepCondition::Never;
                    true
                }
            },
            StepCondition::AfterSuccess(_) | StepCondition::Never => true,
        });
    }
}

pub(crate) async fn execute_plan<S>(
    cx: &ReportContext<S>,
    mut plan: PreparedExecutions<'_, S>,
) -> Result<(), S::Error>
where
    S: ReportScope,
{
    let mut err = None;
    while let Some(execution) = plan.pop_executable() {
        let step_id = execution.step_id;
        if cx.cancel_token().is_cancelled() {
            return Err(S::Error::cancelled());
        }
        match execution.execute(cx).await {
            Ok(()) => plan.notify_result(step_id, StepResult::Success),
            Err(e) => {
                plan.notify_result(step_id, StepResult::Failure);
                err.get_or_insert(e);
            }
        }
    }
    let len = plan.len();
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
