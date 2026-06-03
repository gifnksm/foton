use crate::{
    cli::{
        context::ReportContext,
        reporter::{OperationError as _, ReportScope},
    },
    engine::{ExecutionId, PreparedExecutions},
};

pub(in crate::engine) use self::{install::*, uninstall::*};

mod install;
mod uninstall;

#[derive(Debug, derive_more::From)]
pub(in crate::engine) enum Execution<'db, S>
where
    S: ReportScope,
{
    Install(InstallExecution<'db, S>),
    Uninstall(UninstallExecution<'db>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::engine) enum ExecutionResult {
    Success,
    Failure,
}

impl<S> Execution<'_, S>
where
    S: ReportScope,
{
    async fn execute(self, cx: &ReportContext<S>) -> Result<(), S::Error> {
        match self {
            Self::Install(exec) => exec.execute(cx).await,
            Self::Uninstall(exec) => exec.execute(cx),
        }
    }

    fn exec_id(&self) -> ExecutionId {
        match self {
            Self::Install(exec) => exec.exec_id(),
            Self::Uninstall(exec) => exec.exec_id(),
        }
    }

    pub(in crate::engine) fn can_execute(&self) -> bool {
        match self {
            Execution::Install(exec) => exec.can_execute(),
            Execution::Uninstall(exec) => exec.can_execute(),
        }
    }

    pub(in crate::engine) fn notify_result(
        &mut self,
        exec_id: ExecutionId,
        result: ExecutionResult,
    ) {
        match self {
            Execution::Install(exec) => exec.notify_result(exec_id, result),
            Execution::Uninstall(exec) => exec.notify_result(exec_id, result),
        }
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
        let exec_id = execution.exec_id();
        if cx.cancel_token().is_cancelled() {
            return Err(S::Error::cancelled());
        }
        match execution.execute(cx).await {
            Ok(()) => plan.notify_result(exec_id, ExecutionResult::Success),
            Err(e) => {
                plan.notify_result(exec_id, ExecutionResult::Failure);
                err.get_or_insert(e);
            }
        }
    }
    let len = plan.len();
    if len > 0 {
        cx.reporter().report_info(format_args!(
            "{len} execution(s) were not executed due to unsatisfied conditions."
        ));
    }

    if let Some(err) = err {
        return Err(err);
    }
    Ok(())
}
