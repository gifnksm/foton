use crate::cli::{context::ReportContext, reporter::ReportScope};

pub(crate) use self::{install::*, uninstall::*};

mod install;
mod uninstall;

#[derive(Debug, derive_more::From)]
pub(crate) enum Execution<'db, S>
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
    pub(crate) async fn execute(self, cx: &ReportContext<S>) -> Result<(), S::Error> {
        match self {
            Self::Install(exec) => exec.execute(cx).await,
            Self::Uninstall(exec) => exec.execute(cx),
        }
    }
}
