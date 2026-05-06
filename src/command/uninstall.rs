use std::sync::{Arc, Mutex};

use snafu::Snafu;

use crate::{
    cli::{
        args::UninstallArgs,
        context::RootContext,
        message::BulletList,
        reporter::{NeverReport, OperationError, ReportScope, RootReportScope},
    },
    engine,
};

#[derive(Debug)]
struct UninstallScope {}

impl ReportScope for UninstallScope {
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
    #[snafu(display("failed to uninstall package; see previous messages for details"))]
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
    let db = Arc::new(Mutex::new(db));

    let targets = engine::resolve_uninstall_targets(&cx, &db.lock().unwrap(), pkg_specs)?;

    if targets.is_empty() {
        cx.reporter().report_info(format_args!(
            "no packages need to be uninstalled, nothing to do"
        ));
        return Ok(());
    }

    cx.reporter().report_info(format_args!(
        "Uninstalling the following packages:\n{}",
        BulletList(
            &targets
                .iter()
                .map(|target| format!("{} ({})", target.pkg_id, target.current_state))
                .collect::<Vec<_>>(),
        )
    ));

    let executions = engine::prepare_uninstall(&cx, &db, &targets)?;
    for execution in executions {
        execution.execute(&cx).await?;
    }

    Ok(())
}
