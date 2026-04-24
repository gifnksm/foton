use crate::{
    package::Package,
    platform::windows::primitives::{
        registry::{self, RegisteredFont, RegistryError},
        session::{self, SessionError},
    },
    util::{
        path::AbsolutePath,
        reporter::{
            ReportValue, Reporter, Step, StepReporter, StepResultErrorExt as _,
            StepResultWarnExt as _,
        },
    },
};

#[derive(Debug)]
struct RegistrationStep<'a, S> {
    step: &'a S,
}

impl<S> Step for RegistrationStep<'_, S>
where
    S: Step,
{
    type WarnReportValue = RegistrationWarnReport;
    type ErrorReportValue = RegistrationErrorReport;
    type Error = S::Error;

    fn report_prelude(&self, reporter: &Reporter) {
        reporter.report_step(format_args!("Registering fonts..."));
    }

    fn make_error(&self) -> Self::Error {
        self.step.make_error()
    }
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
enum RegistrationWarnReport {
    #[display(
        "failed to load font into current session: {path}; the font was registered persistently but may not be available until next logon",
        path = path.display()
    )]
    LoadFont {
        path: AbsolutePath,
        #[error(source)]
        source: SessionError,
    },
    #[display(
        "failed to broadcast font change after install; applications may not see the new font immediately"
    )]
    BroadcastFontAfterInstall {
        #[error(source)]
        source: SessionError,
    },
}

impl From<RegistrationWarnReport> for ReportValue<'static> {
    fn from(report: RegistrationWarnReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
enum RegistrationErrorReport {
    #[display("failed to register package fonts in the registry")]
    RegisterFontsInRegistry {
        #[error(source)]
        source: RegistryError,
    },
}

impl From<RegistrationErrorReport> for ReportValue<'static> {
    fn from(report: RegistrationErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

pub(crate) fn register_package_fonts<S>(
    reporter: &StepReporter<'_, S>,
    app_id: &str,
    package: &Package,
) -> Result<(), S::Error>
where
    S: Step,
{
    let reporter = reporter.with_step(RegistrationStep {
        step: reporter.step(),
    });

    let fonts_dir = package.dirs().fonts_dir();
    let registered_fonts = package
        .entries()
        .iter()
        .map(|entry| RegisteredFont::new(entry.title(), fonts_dir.join(entry.file_name())))
        .collect::<Vec<_>>();

    // Report fatal errors at the point of failure so they stay ordered relative to
    // warnings emitted by later best-effort steps in this function. Callers should
    // return the error without reporting it again.
    registry::register_package_fonts(app_id, package.id(), &registered_fonts)
        .map_err(|source| RegistrationErrorReport::RegisterFontsInRegistry { source })
        .report_error(&reporter)?;

    for entry in &registered_fonts {
        session::load_font(entry.path())
            .map_err(|source| {
                let path = entry.path().clone();
                RegistrationWarnReport::LoadFont { path, source }
            })
            .report_warn(&reporter);
    }

    session::broadcast_font_change()
        .map_err(|source| RegistrationWarnReport::BroadcastFontAfterInstall { source })
        .report_warn(&reporter);

    Ok(())
}
