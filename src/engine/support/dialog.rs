use std::{marker::PhantomData, thread};

use dialoguer::Confirm;
use snafu::{ResultExt as _, Snafu};
use tokio::sync::oneshot;

use crate::cli::{
    context::ReportContext,
    reporter::{
        NeverReport, OperationError as _, ReportScope, ReportValue, ScopeResultErrorExt as _,
        SubReportScope,
    },
};

#[derive(Debug)]
struct DialogScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for DialogScope<S>
where
    S: ReportScope,
{
    type NoticeReportValue = NeverReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = DialogErrorReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for DialogScope<S>
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
pub(crate) enum DialogErrorReport {
    #[snafu(display("failed to receive user confirmation"))]
    UserConfirmation { source: dialoguer::Error },
}

impl From<DialogErrorReport> for ReportValue<'static> {
    fn from(report: DialogErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

pub(crate) async fn confirm<S>(cx: &ReportContext<S>, message: &'static str) -> Result<(), S::Error>
where
    S: ReportScope,
{
    let cx = DialogScope::start(cx);

    if cx.global_args().no_confirm {
        return Ok(());
    }

    let (tx, rx) = oneshot::channel();

    // Use `std::thread::spawn` instead of `spawn_blocking` because
    // interactive console input cannot be interrupted, and Tokio waits
    // for blocking tasks during runtime shutdown.
    thread::spawn(move || {
        let res = Confirm::new().with_prompt(message).default(true).interact();
        let _ = tx.send(res);
    });

    let Some(res) = cx.cancel_token().run_until_cancelled(rx).await else {
        return Err(S::Error::cancelled());
    };

    let confirmed = res
        .unwrap()
        .context(UserConfirmationSnafu)
        .report_error(cx.reporter())?;

    if !confirmed {
        return Err(S::Error::cancelled());
    }
    Ok(())
}
