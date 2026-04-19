use color_eyre::eyre;

use crate::{
    cli::message::{error, warn},
    util::error::FormatErrorChain as _,
};

type DynError<'r> = &'r dyn std::error::Error;
type BoxedCallback<'c> = Box<dyn for<'r> FnMut(Report<'r>) + 'c>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum ReportSeverity {
    Error,
    Warn,
}

#[derive(Debug)]
pub(crate) struct Report<'a> {
    severity: ReportSeverity,
    value: DynError<'a>,
}

impl<'a> Report<'a> {
    pub(crate) fn new(severity: ReportSeverity, value: DynError<'a>) -> Self {
        Self { severity, value }
    }

    pub(crate) fn error(value: DynError<'a>) -> Self {
        Self::new(ReportSeverity::Error, value)
    }

    pub(crate) fn warn(value: DynError<'a>) -> Self {
        Self::new(ReportSeverity::Warn, value)
    }

    pub(crate) fn severity(&self) -> ReportSeverity {
        self.severity
    }

    pub(crate) fn value(&self) -> DynError<'a> {
        self.value
    }
}

pub(crate) struct Reporter<'c> {
    callback: BoxedCallback<'c>,
}

impl<'c> Reporter<'c> {
    pub(crate) fn new<C>(callback: C) -> Self
    where
        C: for<'r> FnMut(Report<'r>) + 'c,
    {
        let callback = Box::new(callback) as _;
        Self { callback }
    }

    pub(crate) fn message_reporter() -> Self {
        Self::new(|report| match report.severity() {
            ReportSeverity::Error => error!("{}", report.value().format_error_chain()),
            ReportSeverity::Warn => warn!("{}", report.value().format_error_chain()),
        })
    }

    pub(crate) fn report(&mut self, report: Report<'_>) {
        (self.callback)(report);
    }

    pub(crate) fn report_error(&mut self, value: DynError<'_>) {
        self.report(Report::error(value));
    }

    pub(crate) fn report_warn(&mut self, value: DynError<'_>) {
        self.report(Report::warn(value));
    }
}

pub(crate) trait ReportErrorExt {
    fn report_err_as_error(self, reporter: &mut Reporter<'_>) -> Self;
    fn report_err_as_warn(self, reporter: &mut Reporter<'_>) -> Self;
}

impl<T, E> ReportErrorExt for Result<T, E>
where
    E: std::error::Error,
{
    fn report_err_as_error(self, reporter: &mut Reporter<'_>) -> Self {
        if let Err(err) = &self {
            reporter.report_error(err);
        }
        self
    }

    fn report_err_as_warn(self, reporter: &mut Reporter<'_>) -> Self {
        if let Err(err) = &self {
            reporter.report_warn(err);
        }
        self
    }
}

pub(crate) trait ReportEyreErrorExt {
    fn report_err_as_warn(self, reporter: &mut Reporter<'_>) -> Self;
}

impl<T> ReportEyreErrorExt for Result<T, eyre::Report> {
    fn report_err_as_warn(self, reporter: &mut Reporter<'_>) -> Self {
        if let Err(err) = &self {
            reporter.report_warn(err.as_ref());
        }
        self
    }
}
