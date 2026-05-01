use std::sync::Arc;

use snafu::{ResultExt as _, Snafu};

use crate::{
    cli::context::StepContext,
    package::PackageId,
    platform::windows::primitives::{
        registry::{self, RegistryError},
        session::{self, SessionError},
    },
    util::reporter::{ReportValue, Step, StepResultErrorExt as _, StepResultWarnExt as _},
};

#[derive(Debug)]
struct UnregistrationStep<S> {
    step: Arc<S>,
}

impl<S> Step for UnregistrationStep<S>
where
    S: Step,
{
    type WarnReportValue = UnregistrationWarnReport;
    type ErrorReportValue = UnregistrationErrorReport;
    type Error = S::Error;

    fn make_failed(&self) -> Self::Error {
        self.step.make_failed()
    }
}

#[derive(Debug, Snafu)]
enum UnregistrationWarnReport {
    #[snafu(display("failed to list registered package fonts for the package"))]
    ListInstalledFonts { source: RegistryError },
    #[snafu(display(
        "failed to broadcast font change after uninstall\napplications may continue to use stale font information until refresh"
    ))]
    BroadcastFontAfterUninstall { source: SessionError },
}

impl From<UnregistrationWarnReport> for ReportValue<'static> {
    fn from(report: UnregistrationWarnReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

#[derive(Debug, Snafu)]
enum UnregistrationErrorReport {
    #[snafu(display("failed to unregister package fonts from the registry"))]
    UnregisterFontsFromRegistry { source: RegistryError },
}

impl From<UnregistrationErrorReport> for ReportValue<'static> {
    fn from(report: UnregistrationErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

pub(crate) fn unregister_package_fonts<S>(
    cx: &StepContext<S>,
    pkg_id: &PackageId,
) -> Result<(), S::Error>
where
    S: Step,
{
    let cx = cx.with_step(UnregistrationStep {
        step: Arc::clone(cx.step()),
    });
    let reporter = cx.reporter();
    reporter.report_step(format_args!("Unregistering fonts..."));

    let entries = registry::list_registered_package_fonts(cx.app_id(), pkg_id)
        .context(ListInstalledFontsSnafu)
        .report_warn(reporter);

    if let Some(entries) = entries {
        for entry in entries {
            session::unload_font(entry.path());
        }
    }

    // Report fatal errors at the point of failure so they stay ordered relative to
    // warnings emitted by later best-effort steps in this function. Callers should
    // return the error without reporting it again.
    let res = registry::unregister_package_fonts(cx.app_id(), pkg_id)
        .context(UnregisterFontsFromRegistrySnafu)
        .report_error(reporter);

    let _ = session::broadcast_font_change()
        .context(BroadcastFontAfterUninstallSnafu)
        .report_warn(reporter);

    res
}
