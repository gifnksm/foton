use std::{marker::PhantomData, path::PathBuf};

use snafu::{ResultExt as _, Snafu};

use crate::{
    cli::{
        context::ReportContext,
        reporter::{
            NeverReport, ReportScope, ScopeResultErrorExt as _, ScopeResultNoticeExt as _,
            SubReportScope,
        },
    },
    package::{PackageDirs, PackageFont, PackageId},
    platform::windows::primitives::{
        registry::{self, RegisteredFont, RegisteredFontError, RegistryError},
        session::{self, SessionError},
    },
    util::macros::concat_line,
};

#[derive(Debug, Default)]
struct RegistrationScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for RegistrationScope<S>
where
    S: ReportScope,
{
    type NoticeReportValue = RegistrationNoticeReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = RegistrationErrorReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for RegistrationScope<S> where S: ReportScope {}

#[derive(Debug, Snafu)]
enum RegistrationNoticeReport {
    #[snafu(display(
        concat_line!(
            "failed to load font into current session: {path}",
            "the font was registered persistently but may not be available until next logon",
        ),
        path = path.display(),
    ))]
    LoadFont { path: PathBuf, source: SessionError },
    #[snafu(display(
        concat_line!(
            "failed to broadcast font change after activation",
            "applications may not see the new font immediately"
        ),
    ))]
    BroadcastFontAfterActivation { source: SessionError },
}

#[derive(Debug, Snafu)]
enum RegistrationErrorReport {
    #[snafu(transparent)]
    RegisteredFont { source: RegisteredFontError },
    #[snafu(display("failed to register package fonts in the registry"))]
    RegisterFontsInRegistry { source: RegistryError },
}

pub(crate) fn register_package_fonts<S, I>(
    cx: &ReportContext<S>,
    pkg_id: &PackageId,
    pkg_fonts: I,
) -> Result<(), S::Error>
where
    S: ReportScope,
    I: IntoIterator<Item = PackageFont>,
{
    let cx = RegistrationScope::start_with_report(cx, "Registering fonts...");

    let pkg_dirs = PackageDirs::new(cx.app_dirs(), pkg_id);
    let registered_fonts = pkg_fonts
        .into_iter()
        .map(|pkg_font| RegisteredFont::new(pkg_font.path(&pkg_dirs)))
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
        .report_error(&cx)?;

    // Report fatal errors at the point of failure so they stay ordered relative to
    // warnings emitted by later best-effort steps in this function. Callers should
    // return the error without reporting it again.
    registry::register_package_fonts(cx.app_id(), pkg_id, &registered_fonts)
        .context(RegisterFontsInRegistrySnafu)
        .report_error(&cx)?;

    for font in &registered_fonts {
        let path = font.path();
        session::load_font(path)
            .context(LoadFontSnafu { path })
            .report_notice(&cx);
    }

    session::broadcast_font_change()
        .context(BroadcastFontAfterActivationSnafu)
        .report_notice(&cx);

    Ok(())
}
