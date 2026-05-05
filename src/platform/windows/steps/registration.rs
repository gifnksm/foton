use std::{marker::PhantomData, path::PathBuf};

use snafu::{ResultExt as _, Snafu};

use crate::{
    cli::{
        context::ReportContext,
        reporter::{
            ReportScope, ReportValue, ScopeResultErrorExt as _, ScopeResultWarnExt as _,
            SubReportScope,
        },
    },
    package::Package,
    platform::windows::primitives::{
        registry::{self, RegisteredFont, RegistryError},
        session::{self, SessionError},
    },
};

#[derive(Debug)]
struct RegistrationScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for RegistrationScope<S>
where
    S: ReportScope,
{
    type WarnReportValue = RegistrationWarnReport;
    type ErrorReportValue = RegistrationErrorReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for RegistrationScope<S>
where
    S: ReportScope,
{
    fn new() -> Self {
        Self {
            _base_scope: PhantomData,
        }
    }
}

#[derive(Debug, Snafu)]
enum RegistrationWarnReport {
    #[snafu(display(
        concat!(
            "failed to load font into current session: {path}\n",
            "the font was registered persistently but may not be available until next logon",
        ),
        path = path.display(),
    ))]
    LoadFont { path: PathBuf, source: SessionError },
    #[snafu(display(
        concat!(
            "failed to broadcast font change after install\n",
            "applications may not see the new font immediately"
        ),
    ))]
    BroadcastFontAfterInstall { source: SessionError },
}

impl From<RegistrationWarnReport> for ReportValue<'static> {
    fn from(report: RegistrationWarnReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

#[derive(Debug, Snafu)]
enum RegistrationErrorReport {
    #[snafu(display("failed to register package fonts in the registry"))]
    RegisterFontsInRegistry { source: RegistryError },
}

impl From<RegistrationErrorReport> for ReportValue<'static> {
    fn from(report: RegistrationErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

pub(crate) fn register_package_fonts<S>(
    cx: &ReportContext<S>,
    package: &Package,
) -> Result<(), S::Error>
where
    S: ReportScope,
{
    let cx = RegistrationScope::start_with_report(cx, "Registering fonts...");
    let reporter = cx.reporter();

    let fonts_dir = package.dirs().fonts_dir();
    let registered_fonts = package
        .entries()
        .iter()
        .map(|entry| RegisteredFont::new(entry.title(), fonts_dir.join(entry.file_name())))
        .collect::<Vec<_>>();

    // Report fatal errors at the point of failure so they stay ordered relative to
    // warnings emitted by later best-effort steps in this function. Callers should
    // return the error without reporting it again.
    registry::register_package_fonts(cx.app_id(), package.id(), &registered_fonts)
        .context(RegisterFontsInRegistrySnafu)
        .report_error(reporter)?;

    for entry in &registered_fonts {
        session::load_font(entry.path())
            .context(LoadFontSnafu { path: entry.path() })
            .report_warn(reporter);
    }

    session::broadcast_font_change()
        .context(BroadcastFontAfterInstallSnafu)
        .report_warn(reporter);

    Ok(())
}
