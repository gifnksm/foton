use snafu::Snafu;

use crate::{
    cli::{
        context::RootContext,
        reporter::{NeverReport, ReportScope, RootReportScope},
    },
    command::common,
    package::PackageSpec,
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
}

impl RootReportScope for UninstallScope {
    fn new() -> Self {
        Self {}
    }
}

#[derive(Debug, Snafu)]
pub(crate) enum UninstallError {
    #[snafu(display("failed to uninstall package"))]
    Failed,
}

pub(crate) fn uninstall_package(
    cx: &RootContext,
    pkg_spec: &PackageSpec,
) -> Result<(), UninstallError> {
    let cx = UninstallScope::start_with_report(cx, format_args!("Uninstalling {pkg_spec}..."));
    let reporter = cx.reporter();

    let mut db_lock_file = common::steps::open_db_lock_file(&cx)?;
    let mut db = common::steps::load_database(&cx, &mut db_lock_file)?;

    let Some((_state, manifest)) = common::steps::resolve_spec_in_db(&cx, &db, pkg_spec)? else {
        reporter.report_info(format_args!(
            "no package matches the specified package `{pkg_spec}`; nothing to do"
        ));
        return Ok(());
    };

    let pkg_id = manifest.metadata.id();
    common::steps::uninstall_transaction(&cx, &mut db, &pkg_id)?;

    Ok(())
}
