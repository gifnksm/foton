use crate::{
    cli::context::RootContext,
    command::common,
    package::PackageSpec,
    util::reporter::{NeverReport, Step},
};

#[derive(Debug)]
struct UninstallStep {}

impl Step for UninstallStep {
    type WarnReportValue = NeverReport;
    type ErrorReportValue = NeverReport;
    type Error = UninstallError;

    fn make_failed(&self) -> Self::Error {
        UninstallError::Failed
    }
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum UninstallError {
    #[display("failed to uninstall package")]
    Failed,
}

pub(crate) fn uninstall_package(
    cx: &RootContext,
    pkg_spec: &PackageSpec,
) -> Result<(), UninstallError> {
    let cx = cx.with_step(UninstallStep {});
    let reporter = cx.reporter();
    reporter.report_step(format_args!("Uninstalling {pkg_spec}..."));

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
