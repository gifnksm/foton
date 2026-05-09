use crate::{
    cli::{
        context::ReportContext,
        reporter::{OperationError as _, ReportScope},
    },
    engine::PreparedExecutions,
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
}

pub(crate) async fn execute_plan<S>(
    cx: &ReportContext<S>,
    plan: PreparedExecutions<'_, S>,
) -> Result<(), S::Error>
where
    S: ReportScope,
{
    let mut err = None;
    for execution in plan.into_executions() {
        if cx.cancel_token().is_cancelled() {
            return Err(S::Error::cancelled());
        }
        if let Err(e) = execution.execute(cx).await {
            err.get_or_insert(e);
        }
    }
    if let Some(err) = err {
        return Err(err);
    }
    Ok(())
}
