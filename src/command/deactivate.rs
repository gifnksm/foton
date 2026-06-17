use snafu::Snafu;

use crate::{
    cli::{
        args::DeactivateArgs,
        context::RootContext,
        reporter::{NeverReport, OperationError, ReportScope, RootReportScope},
    },
    engine,
};

#[derive(Debug, Default)]
struct DeactivateScope {}

impl ReportScope for DeactivateScope {
    type NoticeReportValue = NeverReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = NeverReport;
    type Error = DeactivateError;
}

impl RootReportScope for DeactivateScope {}

#[derive(Debug, Snafu)]
pub(crate) enum DeactivateError {
    #[snafu(display("failed to deactivate package(s); see previous messages for details"))]
    Failed,
    #[snafu(display("operation cancelled"))]
    Cancelled,
}

impl OperationError for DeactivateError {
    fn failed() -> Self {
        Self::Failed
    }

    fn cancelled() -> Self {
        Self::Cancelled
    }
}

pub(crate) async fn deactivate_package(
    cx: &RootContext,
    args: &DeactivateArgs,
) -> Result<(), DeactivateError> {
    let DeactivateArgs {
        pkg_specs,
        exit_on_lock,
    } = args;
    let cx = DeactivateScope::start_with_report(
        cx,
        format_args!("Deactivating {} package(s)...", pkg_specs.len()),
    );

    let mut db_lock_file = engine::open_db_lock_file(&cx)?;
    let mut db = engine::load_database(&cx, &mut db_lock_file, *exit_on_lock)?;

    let targets = engine::resolve_deactivate_targets(&cx, &db, pkg_specs)?;
    let plan = engine::plan_deactivate(&targets);
    engine::report_plan(&cx, &plan);

    if !plan.has_side_effects() {
        cx.reporter()
            .report_info(format_args!("already inactive, nothing to do"));
        return Ok(());
    }

    engine::confirm(&cx, "Do you want to continue?").await?;
    engine::execute_plan(&cx, &mut db, plan).await?;

    Ok(())
}
