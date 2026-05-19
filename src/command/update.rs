use std::sync::{Arc, Mutex};

use snafu::Snafu;

use crate::{
    cli::{
        args::UpdateArgs,
        context::RootContext,
        reporter::{NeverReport, OperationError, ReportScope, RootReportScope},
    },
    engine,
};

#[derive(Debug)]
struct UpdateScope {}

impl ReportScope for UpdateScope {
    type NoticeReportValue = NeverReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = NeverReport;
    type Error = UpdateError;
}

impl RootReportScope for UpdateScope {
    fn new() -> Self {
        Self {}
    }
}

#[derive(Debug, Snafu)]
pub(crate) enum UpdateError {
    #[snafu(display("failed to update package(s); see previous messages for details"))]
    Failed,
    #[snafu(display("operation cancelled"))]
    Cancelled,
}

impl OperationError for UpdateError {
    fn failed() -> Self {
        Self::Failed
    }

    fn cancelled() -> Self {
        Self::Cancelled
    }
}

pub(crate) async fn update_package(cx: &RootContext, args: &UpdateArgs) -> Result<(), UpdateError> {
    let UpdateArgs {
        registries,
        pkg_specs,
        pre_release,
    } = args;

    let report = if pkg_specs.is_empty() {
        format_args!("Updating all packages...")
    } else {
        format_args!("Updating {} package(s)...", pkg_specs.len())
    };
    let cx = UpdateScope::start_with_report(cx, report);

    let registries = engine::resolve_registries_by_id(&cx, registries.as_deref())?;

    let mut db_lock_file = engine::open_db_lock_file(&cx)?;
    let db = engine::load_database(&cx, &mut db_lock_file)?;

    let targets = engine::resolve_update_targets(&cx, &db, &registries, pkg_specs, *pre_release)?;
    let plan = engine::plan_install(&db, &targets);
    engine::report_plan(&cx, &plan);

    if !plan.has_side_effects() {
        cx.reporter()
            .report_info(format_args!("already up to date, nothing to do"));
        return Ok(());
    }

    engine::confirm(&cx, "Do you want to continue?").await?;

    let db = Arc::new(Mutex::new(db));
    let prepared_plan = engine::prepare(&cx, &db, &plan)?;
    engine::execute_plan(&cx, prepared_plan).await?;

    Ok(())
}
