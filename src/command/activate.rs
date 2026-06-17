use snafu::Snafu;

use crate::{
    cli::{
        args::ActivateArgs,
        context::RootContext,
        reporter::{NeverReport, OperationError, ReportScope, RootReportScope},
    },
    engine,
};

#[derive(Debug, Default)]
struct ActivateScope {}

impl ReportScope for ActivateScope {
    type NoticeReportValue = NeverReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = NeverReport;
    type Error = ActivateError;
}

impl RootReportScope for ActivateScope {}

#[derive(Debug, Snafu)]
pub(crate) enum ActivateError {
    #[snafu(display("failed to activate package(s); see previous messages for details"))]
    Failed,
    #[snafu(display("operation cancelled"))]
    Cancelled,
}

impl OperationError for ActivateError {
    fn failed() -> Self {
        Self::Failed
    }

    fn cancelled() -> Self {
        Self::Cancelled
    }
}

pub(crate) async fn activate_package(
    cx: &RootContext,
    args: &ActivateArgs,
) -> Result<(), ActivateError> {
    let ActivateArgs {
        pkg_specs,
        exit_on_lock,
    } = args;
    let cx = ActivateScope::start_with_report(
        cx,
        format_args!("Activating {} package(s)...", pkg_specs.len()),
    );

    let mut db_lock_file = engine::open_db_lock_file(&cx)?;
    let mut db = engine::load_database(&cx, &mut db_lock_file, *exit_on_lock)?;

    let targets = engine::resolve_activate_targets(&cx, &db, pkg_specs)?;
    let plan = engine::plan_activate(&db, &targets);
    engine::report_plan(&cx, &plan);

    if !plan.has_side_effects() {
        cx.reporter()
            .report_info(format_args!("already active, nothing to do"));
        return Ok(());
    }

    engine::confirm(&cx, "Do you want to continue?").await?;
    engine::execute_plan(&cx, &mut db, plan).await?;

    Ok(())
}
