use std::sync::Arc;

use snafu::{ResultExt as _, Snafu};

use crate::{
    cli::{
        context::ReportContext,
        reporter::{
            ReportScope, ReportValue, ScopeResultErrorExt as _, ScopeResultWarnExt as _,
            SubReportScope,
        },
    },
    package::PackageId,
    platform::windows::primitives::{
        registry::{self, RegistryError},
        session::{self, SessionError},
    },
};

#[derive(Debug)]
struct UnregistrationScope<S> {
    base_scope: Arc<S>,
}

impl<S> ReportScope for UnregistrationScope<S>
where
    S: ReportScope,
{
    type WarnReportValue = UnregistrationWarnReport;
    type ErrorReportValue = UnregistrationErrorReport;
    type Error = S::Error;

    fn make_failed(&self) -> Self::Error {
        self.base_scope.make_failed()
    }

    fn make_cancelled(&self) -> Self::Error {
        self.base_scope.make_cancelled()
    }
}

impl<S> SubReportScope<S> for UnregistrationScope<S>
where
    S: ReportScope,
{
    fn new(base_scope: Arc<S>) -> Self {
        Self { base_scope }
    }
}

#[derive(Debug, Snafu)]
enum UnregistrationWarnReport {
    #[snafu(display("failed to list registered package fonts for the package"))]
    ListInstalledFonts { source: RegistryError },
    #[snafu(display(
        concat!(
            "failed to broadcast font change after uninstall\n",
            "applications may continue to use stale font information until refresh",
        )
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
    cx: &ReportContext<S>,
    pkg_id: &PackageId,
) -> Result<(), S::Error>
where
    S: ReportScope,
{
    let cx = UnregistrationScope::start_with_report(cx, "Unregistering fonts...");
    let reporter = cx.reporter();

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
