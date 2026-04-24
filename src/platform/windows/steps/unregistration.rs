use crate::{
    package::PackageId,
    platform::windows::primitives::{
        registry::{self, RegistryError},
        session::{self, SessionError},
    },
    util::reporter::{
        ReportValue, Reporter, Step, StepReporter, StepResultErrorExt as _, StepResultWarnExt as _,
    },
};

#[derive(Debug)]
struct UnregistrationStep<'a, S> {
    step: &'a S,
}

impl<S> Step for UnregistrationStep<'_, S>
where
    S: Step,
{
    type WarnReportValue = UnregistrationWarnReport;
    type ErrorReportValue = UnregistrationErrorReport;
    type Error = S::Error;

    fn report_prelude(&self, reporter: &Reporter) {
        reporter.report_step(format_args!("Unregistering fonts..."));
    }

    fn make_failed(&self) -> Self::Error {
        self.step.make_failed()
    }
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
enum UnregistrationWarnReport {
    #[display("failed to list registered package fonts for the package")]
    ListInstalledFonts {
        #[error(source)]
        source: RegistryError,
    },
    #[display(
        "failed to broadcast font change after uninstall; applications may continue to use stale font information until refresh"
    )]
    BroadcastFontAfterUninstall {
        #[error(source)]
        source: SessionError,
    },
}

impl From<UnregistrationWarnReport> for ReportValue<'static> {
    fn from(report: UnregistrationWarnReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
enum UnregistrationErrorReport {
    #[display("failed to unregister package fonts from the registry")]
    UnregisterFontsFromRegistry {
        #[error(source)]
        source: RegistryError,
    },
}

impl From<UnregistrationErrorReport> for ReportValue<'static> {
    fn from(report: UnregistrationErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

pub(crate) fn unregister_package_fonts<S>(
    reporter: &StepReporter<'_, S>,
    app_id: &str,
    pkg_id: &PackageId,
) -> Result<(), S::Error>
where
    S: Step,
{
    let reporter = reporter.with_step(UnregistrationStep {
        step: reporter.step(),
    });

    let entries = registry::list_registered_package_fonts(app_id, pkg_id)
        .map_err(|source| UnregistrationWarnReport::ListInstalledFonts { source })
        .report_warn(&reporter);

    if let Some(entries) = entries {
        for entry in entries {
            session::unload_font(entry.path());
        }
    }

    // Report fatal errors at the point of failure so they stay ordered relative to
    // warnings emitted by later best-effort steps in this function. Callers should
    // return the error without reporting it again.
    let res = registry::unregister_package_fonts(app_id, pkg_id)
        .map_err(|source| UnregistrationErrorReport::UnregisterFontsFromRegistry { source })
        .report_error(&reporter);

    let _ = session::broadcast_font_change()
        .map_err(|source| UnregistrationWarnReport::BroadcastFontAfterUninstall { source })
        .report_warn(&reporter);

    res
}
