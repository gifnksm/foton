use std::{
    fmt::{self, Debug, Display},
    sync::{Arc, LazyLock, Mutex},
};

use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressState, ProgressStyle};

use crate::{
    cli::message::{error, info, step, warn},
    util::error::FormatErrorChain as _,
};

type SharedCallback = Arc<dyn for<'r> Fn(Report<'r>) + Send + Sync + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum ReportSeverity {
    Step,
    Info,
    Error,
    Warn,
}

#[derive(Debug, derive_more::From)]
pub(crate) enum ReportValue<'a> {
    FmtArgs(#[from] fmt::Arguments<'a>),
    BoxedError(#[from] Box<dyn std::error::Error + Send + Sync + 'a>),
}

impl Display for ReportValue<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReportValue::FmtArgs(args) => write!(f, "{args}"),
            ReportValue::BoxedError(err) => write!(f, "{}", err.format_error_chain()),
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

    pub(crate) fn value(&self) -> &ReportValue<'_> {
        &self.value
    }
}

#[derive(Clone)]
pub(crate) struct Reporter {
    multi_progress_bar: Arc<Mutex<MultiProgress>>,
    callback: SharedCallback,
}

impl Debug for Reporter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Reporter")
            .field("multi_progress_bar", &"<MultiProgress>")
            .field("callback", &"<callback>")
            .finish()
    }
}

impl Reporter {
    fn new<C>(callback: C) -> Self
    where
        C: for<'r> Fn(Report<'r>) + Send + Sync + 'static,
    {
        let pb = Arc::new(Mutex::new(MultiProgress::with_draw_target(
            ProgressDrawTarget::stderr(),
        )));
        let callback = Arc::new(callback) as _;
        Self {
            multi_progress_bar: pb,
            callback,
        }
    }

    pub(crate) fn message_reporter() -> Self {
        Self::new(|report| match report.severity() {
            ReportSeverity::Step => step!("{}", report.value()),
            ReportSeverity::Info => info!("{}", report.value()),
            ReportSeverity::Error => error!("{}", report.value()),
            ReportSeverity::Warn => warn!("{}", report.value()),
        })
    }

    pub(crate) fn with_step<S>(&self, step: S) -> StepReporter<'_, S>
    where
        S: Step,
    {
        step.report_prelude(self);
        StepReporter {
            reporter: self,
            step,
        }
    }

    pub(crate) fn report(&self, report: Report<'_>) {
        let mpb = self.multi_progress_bar.lock().unwrap().clone();
        mpb.suspend(|| (self.callback)(report));
    }

    pub(crate) fn report_step<'a, V>(&self, value: V)
    where
        V: Into<ReportValue<'a>>,
    {
        self.report(Report::step(value));
    }

    pub(crate) fn report_info<'a, V>(&self, value: V)
    where
        V: Into<ReportValue<'a>>,
    {
        self.report(Report::info(value));
    }

    pub(crate) fn report_error<'a, V>(&self, value: V)
    where
        V: Into<ReportValue<'a>>,
    {
        self.report(Report::error(value));
    }

    pub(crate) fn report_warn<'a, V>(&self, value: V)
    where
        V: Into<ReportValue<'a>>,
    {
        self.report(Report::warn(value));
    }
}

#[derive(Debug)]
pub(crate) enum NeverReport {}

impl From<NeverReport> for ReportValue<'_> {
    fn from(report: NeverReport) -> Self {
        match report {}
    }
}

pub(crate) trait Step {
    type WarnReportValue: Into<ReportValue<'static>>;
    type ErrorReportValue: Into<ReportValue<'static>>;
    type Error;

    fn report_prelude(&self, reporter: &Reporter);
    fn make_failed(&self) -> Self::Error;
}

#[derive(Debug)]
pub(crate) struct StepReporter<'a, S> {
    reporter: &'a Reporter,
    step: S,
}

impl<S> StepReporter<'_, S>
where
    S: Step,
{
    pub(crate) fn step(&self) -> &S {
        &self.step
    }

    pub(crate) fn with_step<T>(&self, step: T) -> StepReporter<'_, T>
    where
        T: Step,
    {
        step.report_prelude(self.reporter);
        StepReporter {
            reporter: self.reporter,
            step,
        }
    }

    pub(crate) fn report_info<'a, V>(&self, report: V)
    where
        V: Into<ReportValue<'a>>,
    {
        self.reporter.report_info(report);
    }

    pub(crate) fn report_warn(&self, report: S::WarnReportValue) {
        self.reporter.report_warn(report);
    }

    pub(crate) fn report_error(&self, report: S::ErrorReportValue) -> S::Error {
        self.reporter.report_error(report);
        self.step.make_failed()
    }

    pub(crate) fn download_progress_bar(&self, len: Option<u64>) -> ProgressBar {
        static KNOWN_LEN_STYLE: LazyLock<ProgressStyle> = LazyLock::new(|| {
            ProgressStyle::with_template(
                "{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})",
            )
            .unwrap()
            .with_key("eta", |state: &ProgressState, w: &mut dyn fmt::Write| {
                let _ = write!(w, "{:.1}s", state.eta().as_secs_f64());
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
        let pb = ProgressBar::with_draw_target(len, ProgressDrawTarget::stderr()).with_style(style);
        self.reporter.multi_progress_bar.lock().unwrap().add(pb)
    }

    pub(crate) async fn with_download_progress_bar<T, E, F>(
        &self,
        len: Option<u64>,
        f: F,
    ) -> Result<T, E>
    where
        F: AsyncFnOnce(&ProgressBar) -> Result<T, E>,
    {
        let pb = self.download_progress_bar(len);
        let res = f(&pb).await;
        match &res {
            Ok(_) => pb.finish(),
            Err(_) => pb.abandon(),
        }
        res
    }
}

pub(crate) trait StepResultWarnExt<S>
where
    S: Step,
{
    type Item;

    fn report_warn(self, reporter: &StepReporter<'_, S>) -> Option<Self::Item>;
}

impl<S, T, E> StepResultWarnExt<S> for Result<T, E>
where
    S: Step<WarnReportValue = E>,
{
    type Item = T;

    fn report_warn(self, reporter: &StepReporter<'_, S>) -> Option<Self::Item> {
        self.map_err(|err| reporter.report_warn(err)).ok()
    }
}

pub(crate) trait StepResultErrorExt<S>
where
    S: Step,
{
    type Item;

    fn report_error(self, reporter: &StepReporter<'_, S>) -> Result<Self::Item, S::Error>;
}

impl<S, T, E> StepResultErrorExt<S> for Result<T, E>
where
    S: Step<ErrorReportValue = E>,
{
    type Item = T;

    fn report_error(self, reporter: &StepReporter<'_, S>) -> Result<Self::Item, S::Error> {
        self.map_err(|err| reporter.report_error(err))
    }
}
