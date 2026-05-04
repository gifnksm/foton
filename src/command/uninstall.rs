use std::sync::{Arc, Mutex};

use snafu::Snafu;

use crate::{
    cli::{
        args::UninstallArgs,
        context::RootContext,
        reporter::{NeverReport, ReportScope, RootReportScope},
    },
    command::common,
    engine,
};

#[derive(Debug)]
struct UninstallScope {}

impl ReportScope for UninstallScope {
    type WarnReportValue = NeverReport;
    type ErrorReportValue = NeverReport;
    type Error = UninstallError;

    fn make_failed(&self) -> Self::Error {
        UninstallError::Failed
    }

    fn make_cancelled(&self) -> Self::Error {
        UninstallError::Cancelled
    }
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

pub(crate) fn uninstall_package(
    cx: &RootContext,
    args: &UninstallArgs,
) -> Result<(), UninstallError> {
    let UninstallArgs { pkg_specs } = args;

    let cx = UninstallScope::start_with_report(
        cx,
        format_args!("Uninstalling {} package(s)...", pkg_specs.len()),
    );
    let reporter = cx.reporter();

    let mut db_lock_file = common::steps::open_db_lock_file(&cx)?;
    let db = common::steps::load_database(&cx, &mut db_lock_file)?;
    let db = Arc::new(Mutex::new(db));

    for pkg_spec in pkg_specs {
        let Some(pkg_id) = common::steps::resolve_spec_in_db(&cx, &db.lock().unwrap(), pkg_spec)?
            .map(|(_state, manifest)| manifest.metadata.id())
        else {
            reporter.report_info(format_args!(
                "no package matches the specified package `{pkg_spec}`; nothing to do"
            ));
            continue;
        };

        engine::execute_uninstall(&cx, &db, &pkg_id)?;
    }

    Ok(())
}
