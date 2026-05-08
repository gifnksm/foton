use std::sync::{Arc, Mutex};

use snafu::Snafu;

use crate::{
    cli::{
        args::InstallArgs,
        context::RootContext,
        reporter::{NeverReport, OperationError, ReportScope, RootReportScope},
    },
    engine,
};

#[derive(Debug)]
struct InstallScope {}

impl ReportScope for InstallScope {
    type WarnReportValue = NeverReport;
    type ErrorReportValue = NeverReport;
    type Error = InstallError;
}

impl RootReportScope for InstallScope {
    fn new() -> Self {
        Self {}
    }
}

#[derive(Debug, Snafu)]
pub(crate) enum InstallError {
    #[snafu(display("failed to install package; see previous messages for details"))]
    Failed,
    #[snafu(display("operation cancelled"))]
    Cancelled,
}

impl OperationError for InstallError {
    fn failed() -> Self {
        Self::Failed
    }

    fn cancelled() -> Self {
        Self::Cancelled
    }
}

pub(crate) async fn install_package(
    cx: &RootContext,
    args: &InstallArgs,
) -> Result<(), InstallError> {
    let InstallArgs {
        registries,
        pkg_specs,
    } = args;

    let cx = InstallScope::start_with_report(
        cx,
        format_args!("Installing {} package(s)...", pkg_specs.len()),
    );

    let registries = engine::resolve_registries_by_id(&cx, registries.as_deref())?;

    let mut db_lock_file = engine::open_db_lock_file(&cx)?;
    let db = engine::load_database(&cx, &mut db_lock_file)?;

    let targets = engine::resolve_install_targets(&cx, &db, &registries, pkg_specs)?;
    let plan = engine::plan_install(&db, &targets);
    engine::report_plan(&cx, &plan);

    if !plan.has_side_effects() {
        cx.reporter().report_info(format_args!(
            "all specified packages are already installed, nothing to do"
        ));
        return Ok(());
    }

    engine::confirm(&cx, "Do you want to continue?").await?;

    let db = Arc::new(Mutex::new(db));
    let executions = engine::prepare(&cx, &db, &plan)?;
    for execution in executions {
        execution.execute(&cx).await?;
    }

    Ok(())
}
