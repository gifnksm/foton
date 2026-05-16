use std::sync::{Arc, Mutex};

use snafu::Snafu;

use crate::{
    cli::{
        args::UninstallArgs,
        context::RootContext,
        reporter::{NeverReport, OperationError, ReportScope, RootReportScope},
    },
    engine,
};

#[derive(Debug)]
struct UninstallScope {}

impl ReportScope for UninstallScope {
    type NoticeReportValue = NeverReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = NeverReport;
    type Error = UninstallError;
}

impl RootReportScope for UninstallScope {
    fn new() -> Self {
        Self {}
    }
}

#[derive(Debug, Snafu)]
pub(crate) enum UninstallError {
    #[snafu(display("failed to uninstall package(s); see previous messages for details"))]
    Failed,
    #[snafu(display("operation cancelled"))]
    Cancelled,
}

impl OperationError for UninstallError {
    fn failed() -> Self {
        Self::Failed
    }

    fn cancelled() -> Self {
        Self::Cancelled
    }
}

pub(crate) async fn uninstall_package(
    cx: &RootContext,
    args: &UninstallArgs,
) -> Result<(), UninstallError> {
    let UninstallArgs { pkg_specs } = args;

    let cx = UninstallScope::start_with_report(
        cx,
        format_args!("Uninstalling {} package(s)...", pkg_specs.len()),
    );

    let mut db_lock_file = engine::open_db_lock_file(&cx)?;
    let db = engine::load_database(&cx, &mut db_lock_file)?;

    let targets = engine::resolve_uninstall_targets(&cx, &db, pkg_specs)?;
    let plan = engine::plan_uninstall(&db, &targets);
    engine::report_plan(&cx, &plan);

    if !plan.has_side_effects() {
        cx.reporter()
            .report_info(format_args!("already uninstalled, nothing to do"));
        return Ok(());
    }

    engine::confirm(&cx, "Do you want to continue?").await?;

    let db = Arc::new(Mutex::new(db));
    let prepared_plan = engine::prepare(&cx, &db, &plan)?;
    engine::execute_plan(&cx, prepared_plan).await?;

    Ok(())
}
