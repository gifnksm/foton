use std::sync::{Arc, Mutex};

use snafu::Snafu;

use crate::{
    cli::{
        args::RepairArgs,
        context::RootContext,
        reporter::{NeverReport, OperationError, ReportScope, RootReportScope},
    },
    engine,
};

#[derive(Debug)]
struct RepairScope {}

impl ReportScope for RepairScope {
    type NoticeReportValue = NeverReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = NeverReport;
    type Error = RepairError;
}

impl RootReportScope for RepairScope {
    fn new() -> Self {
        Self {}
    }
}

#[derive(Debug, Snafu)]
pub(crate) enum RepairError {
    #[snafu(display("failed to repair package(s); see previous messages for details"))]
    Failed,
    #[snafu(display("operation cancelled"))]
    Cancelled,
}

impl OperationError for RepairError {
    fn failed() -> Self {
        Self::Failed
    }

    fn cancelled() -> Self {
        Self::Cancelled
    }
}

pub(crate) async fn repair_package(cx: &RootContext, args: &RepairArgs) -> Result<(), RepairError> {
    let RepairArgs { pkg_specs } = args;

    let report = if pkg_specs.is_empty() {
        format_args!("Repairing all packages...")
    } else {
        format_args!("Repairing {} package(s)...", pkg_specs.len())
    };
    let cx = RepairScope::start_with_report(cx, report);

    let mut db_lock_file = engine::open_db_lock_file(&cx)?;
    let db = engine::load_database(&cx, &mut db_lock_file)?;

    let targets = if pkg_specs.is_empty() {
        engine::resolve_all_repair_targets(&cx, &db)
    } else {
        engine::resolve_repair_targets(&cx, &db, pkg_specs)
    };
    let plan = engine::plan_repair(&db, &targets);
    engine::report_plan(&cx, &plan);

    if !plan.has_side_effects() {
        cx.reporter()
            .report_info(format_args!("already repaired, nothing to do"));
        return Ok(());
    }

    engine::confirm(&cx, "Do you want to continue?").await?;

    let db = Arc::new(Mutex::new(db));
    let prepared_plan = engine::prepare(&cx, &db, &plan)?;
    engine::execute_plan(&cx, prepared_plan).await?;

    Ok(())
}
