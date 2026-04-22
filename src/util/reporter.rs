use std::{
    fmt::{self, Display},
    sync::LazyLock,
};

use color_eyre::eyre;
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressState, ProgressStyle};

use crate::{
    cli::message::{error, info, step, warn},
    util::error::FormatErrorChain as _,
};

type DynError<'r> = &'r dyn std::error::Error;
type BoxedCallback<'c> = Box<dyn for<'r> FnMut(Report<'r>) + 'c>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum ReportSeverity {
    Step,
    Info,
    Error,
    Warn,
}

#[derive(Debug, Clone, Copy, derive_more::From)]
pub(crate) enum ReportValue<'a> {
    FmtArgs(#[from] fmt::Arguments<'a>),
    DynError(#[from] DynError<'a>),
}

impl Display for ReportValue<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReportValue::FmtArgs(args) => write!(f, "{args}"),
            ReportValue::DynError(err) => write!(f, "{}", err.format_error_chain()),
        }
    }
}

#[derive(Debug)]
pub(crate) struct Report<'a> {
    severity: ReportSeverity,
    value: ReportValue<'a>,
}

impl<'a> Report<'a> {
    pub(crate) fn new<V>(severity: ReportSeverity, value: V) -> Self
    where
        V: Into<ReportValue<'a>>,
    {
        let value = value.into();
        Self { severity, value }
    }

    pub(crate) fn info<V>(value: V) -> Self
    where
        V: Into<ReportValue<'a>>,
    {
        Self::new(ReportSeverity::Info, value)
    }

    pub(crate) fn step<V>(value: V) -> Self
    where
        V: Into<ReportValue<'a>>,
    {
        Self::new(ReportSeverity::Step, value)
    }

    pub(crate) fn error<V>(value: V) -> Self
    where
        V: Into<ReportValue<'a>>,
    {
        Self::new(ReportSeverity::Error, value)
    }

    pub(crate) fn warn<V>(value: V) -> Self
    where
        V: Into<ReportValue<'a>>,
    {
        Self::new(ReportSeverity::Warn, value)
    }

    pub(crate) fn severity(&self) -> ReportSeverity {
        self.severity
    }

    pub(crate) fn value(&self) -> ReportValue<'_> {
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
            ReportSeverity::Step => step!("{}", report.value()),
            ReportSeverity::Info => info!("{}", report.value()),
            ReportSeverity::Error => error!("{}", report.value()),
            ReportSeverity::Warn => warn!("{}", report.value()),
        })
    }

    pub(crate) fn report(&mut self, report: Report<'_>) {
        (self.callback)(report);
    }

    pub(crate) fn report_step<'a, V>(&mut self, value: V)
    where
        V: Into<ReportValue<'a>>,
    {
        self.report(Report::step(value));
    }

    pub(crate) fn report_info<'a, V>(&mut self, value: V)
    where
        V: Into<ReportValue<'a>>,
    {
        self.report(Report::info(value));
    }

    pub(crate) fn report_error<'a, V>(&mut self, value: V)
    where
        V: Into<ReportValue<'a>>,
    {
        self.report(Report::error(value));
    }

    pub(crate) fn report_warn<'a, V>(&mut self, value: V)
    where
        V: Into<ReportValue<'a>>,
    {
        self.report(Report::warn(value));
    }

    #[expect(clippy::unused_self)]
    pub(crate) fn download_progress_bar(&self, len: Option<u64>) -> ProgressBar {
        static KNOWN_LEN_STYLE: LazyLock<ProgressStyle> = LazyLock::new(|| {
            ProgressStyle::with_template(
                "{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})",
            )
            .unwrap()
            .with_key("eta", |state: &ProgressState, w: &mut dyn fmt::Write| {
                write!(w, "{:.1}s", state.eta().as_secs_f64()).unwrap();
            })
            .progress_chars("#>-")
        });
        static UNKNOWN_LEN_STYLE: LazyLock<ProgressStyle> = LazyLock::new(|| {
            ProgressStyle::with_template("{spinner:.green} [{elapsed_precise}] {bytes} downloaded")
                .unwrap()
        });

        let style = match len {
            Some(_) => KNOWN_LEN_STYLE.clone(),
            None => UNKNOWN_LEN_STYLE.clone(),
        };
        ProgressBar::with_draw_target(len, ProgressDrawTarget::stderr()).with_style(style)
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
            reporter.report_error(err as DynError<'_>);
        }
        self
    }

    fn report_err_as_warn(self, reporter: &mut Reporter<'_>) -> Self {
        if let Err(err) = &self {
            reporter.report_warn(err as DynError<'_>);
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
