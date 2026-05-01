use std::{
    fmt::{self, Debug, Display},
    sync::{Arc, LazyLock, Mutex},
};

use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressState, ProgressStyle};

use crate::{
    cli::{
        context::{ReportContext, RootContext},
        message,
    },
    util::error::FormatErrorChain as _,
};

type SharedCallback = Arc<dyn for<'r> Fn(Report<'r>) + Send + Sync + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum ReportSeverity {
    Scope,
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

    pub(crate) fn scope<V>(value: V) -> Self
    where
        V: Into<ReportValue<'a>>,
    {
        Self::new(ReportSeverity::Scope, value)
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
pub(crate) struct RootReporter {
    multi_progress_bar: Arc<Mutex<MultiProgress>>,
    callback: SharedCallback,
}

impl Debug for RootReporter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Reporter")
            .field("multi_progress_bar", &"<MultiProgress>")
            .field("callback", &"<callback>")
            .finish()
    }
}

impl RootReporter {
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
            ReportSeverity::Scope => message::scope!("{}", report.value()),
            ReportSeverity::Info => message::info!("{}", report.value()),
            ReportSeverity::Error => message::error!("{}", report.value()),
            ReportSeverity::Warn => message::warn!("{}", report.value()),
        })
    }

    pub(crate) fn with_scope<S>(&self, scope: S) -> ScopedReporter<S>
    where
        S: ReportScope,
    {
        ScopedReporter {
            root_reporter: self.clone(),
            scope: Arc::new(scope),
        }
    }

    pub(crate) fn report(&self, report: Report<'_>) {
        let mpb = self.multi_progress_bar.lock().unwrap().clone();
        mpb.suspend(|| (self.callback)(report));
    }

    pub(crate) fn report_scope<'a, V>(&self, value: V)
    where
        V: Into<ReportValue<'a>>,
    {
        self.report(Report::scope(value));
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

pub(crate) trait ReportScope: Debug {
    type WarnReportValue: Into<ReportValue<'static>>;
    type ErrorReportValue: Into<ReportValue<'static>>;
    type Error;

    fn make_failed(&self) -> Self::Error;
}

pub(crate) trait RootReportScope: ReportScope {
    fn new() -> Self;

    fn start(base_cx: &RootContext) -> ReportContext<Self>
    where
        Self: Sized,
    {
        base_cx.with_scope(Self::new())
    }

    fn start_with_report<M>(base_cx: &RootContext, message: M) -> ReportContext<Self>
    where
        Self: Sized,
        M: Display,
    {
        let cx = Self::start(base_cx);
        cx.reporter().report_scope(format_args!("{message}"));
        cx
    }
}

pub(crate) trait SubReportScope<S>: ReportScope {
    fn new(base_scope: Arc<S>) -> Self;

    fn start(base_cx: &ReportContext<S>) -> ReportContext<Self>
    where
        Self: Sized,
        S: ReportScope<Error = Self::Error>,
    {
        let base_scope = Arc::clone(base_cx.scope());
        base_cx.with_scope(Self::new(base_scope))
    }

    fn start_with_report<M>(base_cx: &ReportContext<S>, message: M) -> ReportContext<Self>
    where
        Self: Sized,
        S: ReportScope<Error = Self::Error>,
        M: Display,
    {
        let cx = Self::start(base_cx);
        cx.reporter().report_scope(format_args!("{message}"));
        cx
    }
}

#[derive(Debug)]
pub(crate) struct ScopedReporter<S> {
    root_reporter: RootReporter,
    scope: Arc<S>,
}

impl<S> Clone for ScopedReporter<S> {
    fn clone(&self) -> Self {
        Self {
            root_reporter: self.root_reporter.clone(),
            scope: Arc::clone(&self.scope),
        }
    }
}

impl<S> ScopedReporter<S>
where
    S: ReportScope,
{
    pub(crate) fn scope(&self) -> &Arc<S> {
        &self.scope
    }

    pub(crate) fn with_scope<T>(&self, scope: T) -> ScopedReporter<T>
    where
        T: ReportScope<Error = S::Error>,
    {
        ScopedReporter {
            root_reporter: self.root_reporter.clone(),
            scope: Arc::new(scope),
        }
    }

    pub(crate) fn report_scope<'a, V>(&self, report: V)
    where
        V: Into<ReportValue<'a>>,
    {
        self.root_reporter.report_scope(report);
    }

    pub(crate) fn report_info<'a, V>(&self, report: V)
    where
        V: Into<ReportValue<'a>>,
    {
        self.root_reporter.report_info(report);
    }

    pub(crate) fn report_warn(&self, report: S::WarnReportValue) {
        self.root_reporter.report_warn(report);
    }

    pub(crate) fn report_error(&self, report: S::ErrorReportValue) -> S::Error {
        self.root_reporter.report_error(report);
        self.scope.make_failed()
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
        self.root_reporter
            .multi_progress_bar
            .lock()
            .unwrap()
            .add(pb)
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

pub(crate) trait ScopeResultWarnExt<S>
where
    S: ReportScope,
{
    type Item;

    fn report_warn(self, reporter: &ScopedReporter<S>) -> Option<Self::Item>;
}

impl<S, T, E> ScopeResultWarnExt<S> for Result<T, E>
where
    S: ReportScope<WarnReportValue = E>,
{
    type Item = T;

    fn report_warn(self, reporter: &ScopedReporter<S>) -> Option<Self::Item> {
        self.map_err(|err| reporter.report_warn(err)).ok()
    }
}

pub(crate) trait ScopeResultErrorExt<S>
where
    S: ReportScope,
{
    type Item;

    fn report_error(self, reporter: &ScopedReporter<S>) -> Result<Self::Item, S::Error>;
}

impl<S, T, E> ScopeResultErrorExt<S> for Result<T, E>
where
    S: ReportScope<ErrorReportValue = E>,
{
    type Item = T;

    fn report_error(self, reporter: &ScopedReporter<S>) -> Result<Self::Item, S::Error> {
        self.map_err(|err| reporter.report_error(err))
    }
}
