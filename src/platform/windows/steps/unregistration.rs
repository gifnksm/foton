use std::marker::PhantomData;

use snafu::{ResultExt as _, Snafu};

use crate::{
    cli::{
        context::ReportContext,
        reporter::{
            NeverReport, ReportScope, ScopeResultErrorExt as _, ScopeResultNoticeExt as _,
            SubReportScope,
        },
    },
    package::PackageId,
    platform::windows::primitives::{
        registry::{self, RegistryError},
        session::{self, SessionError},
    },
    util::macros::concat_line,
};

#[derive(Debug, Default)]
struct UnregistrationScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for UnregistrationScope<S>
where
    S: ReportScope,
{
    type NoticeReportValue = UnregistrationNoticeReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = UnregistrationErrorReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for UnregistrationScope<S> where S: ReportScope {}

#[derive(Debug, Snafu)]
enum UnregistrationNoticeReport {
    #[snafu(display("failed to list registered package fonts for the package"))]
    ListInstalledFonts { source: RegistryError },
    #[snafu(display(
        concat_line!(
            "failed to broadcast font change after font unregistration",
            "applications may continue to use stale font information until refresh",
        )
    ))]
    BroadcastFontAfterUninstall { source: SessionError },
}

#[derive(Debug, Snafu)]
enum UnregistrationErrorReport {
    #[snafu(display("failed to unregister package fonts from the registry"))]
    UnregisterFontsFromRegistry { source: RegistryError },
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum UnregistrationIntent {
    CleanupBeforeInstall,
    CleanupBeforeActivation,
    RollbackAfterActivationFailure,
    DeactivationStep,
}

pub(crate) fn unregister_package_fonts<S>(
    cx: &ReportContext<S>,
    pkg_id: &PackageId,
    intent: UnregistrationIntent,
) -> Result<(), S::Error>
where
    S: ReportScope,
{
    let cx = match intent {
        UnregistrationIntent::CleanupBeforeInstall
        | UnregistrationIntent::CleanupBeforeActivation
        | UnregistrationIntent::RollbackAfterActivationFailure => UnregistrationScope::start(cx),
        UnregistrationIntent::DeactivationStep => {
            UnregistrationScope::start_with_report(cx, "Unregistering fonts...")
        }
    };
    let reporter = cx.reporter();

    let entries = registry::list_registered_package_fonts(cx.app_id(), pkg_id)
        .context(ListInstalledFontsSnafu)
        .report_notice(reporter);

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

    session::broadcast_font_change()
        .context(BroadcastFontAfterUninstallSnafu)
        .report_notice(reporter);

    res
}
