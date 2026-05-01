use std::{path::PathBuf, sync::Arc};

use snafu::{ResultExt as _, Snafu};

use crate::{
    cli::context::StepContext,
    package::Package,
    platform::windows::primitives::{
        registry::{self, RegisteredFont, RegistryError},
        session::{self, SessionError},
    },
    util::reporter::{ReportValue, Step, StepResultErrorExt as _, StepResultWarnExt as _},
};

#[derive(Debug)]
struct RegistrationStep<S> {
    step: Arc<S>,
}

impl<S> Step for RegistrationStep<S>
where
    S: Step,
{
    type WarnReportValue = RegistrationWarnReport;
    type ErrorReportValue = RegistrationErrorReport;
    type Error = S::Error;

    fn make_failed(&self) -> Self::Error {
        self.step.make_failed()
    }
}

#[derive(Debug, Snafu)]
enum RegistrationWarnReport {
    #[snafu(display(
        "failed to load font into current session: {path}\nthe font was registered persistently but may not be available until next logon",
        path = path.display()
    ))]
    LoadFont { path: PathBuf, source: SessionError },
    #[snafu(display(
        "failed to broadcast font change after install\napplications may not see the new font immediately"
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
    cx: &StepContext<S>,
    package: &Package,
) -> Result<(), S::Error>
where
    S: Step,
{
    let cx = cx.with_step(RegistrationStep {
        step: Arc::clone(cx.step()),
    });
    let reporter = cx.reporter();
    reporter.report_step(format_args!("Registering fonts..."));

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
