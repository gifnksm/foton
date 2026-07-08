use std::{
    collections::HashMap,
    fmt::{self, Display},
    mem,
    panic::{self, PanicHookInfo, UnwindSafe},
    process,
    sync::{Arc, LazyLock, Mutex},
    thread::{self, Thread, ThreadId},
};

use chrono::{DateTime, Utc};
use color_eyre::eyre::{self, WrapErr as _, ensure};
use serde::{Deserialize, Serialize};

use crate::scenario::Scenario;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) struct RunId {
    timestamp: DateTime<Utc>,
    pid: u32,
}

impl Display for RunId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}-{}",
            self.timestamp.format("%Y%m%d-%H%M%S-%3f"),
            self.pid,
        )
    }
}

impl RunId {
    pub(crate) fn new() -> Self {
        Self {
            timestamp: Utc::now(),
            pid: process::id(),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RunKind {
    Noop,
    Test,
    Scenario(Scenario),
}

impl Display for RunKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Noop => write!(f, "plain"),
            Self::Test => write!(f, "test"),
            Self::Scenario(scenario) => write!(f, "scenario/{scenario}"),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, derive_more::IsVariant)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RunOutcome {
    Success,
    Failure { error: String },
}

impl From<&eyre::Result<()>> for RunOutcome {
    fn from(value: &eyre::Result<()>) -> Self {
        match value {
            Ok(()) => RunOutcome::Success,
            Err(err) => RunOutcome::Failure {
                error: format!("{err:?}"),
            },
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct ExecResult {
    pub(crate) timestamp: DateTime<Utc>,
    pub(crate) caller: String,
    pub(crate) name: String,
    pub(crate) arguments: Vec<String>,
    pub(crate) success: bool,
    pub(crate) exit_status: String,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

impl ExecResult {
    #[track_caller]
    pub(crate) fn ensure_success(&self) -> eyre::Result<&Self> {
        ensure!(
            self.success,
            "{} failed with exit status {}. stderr:\n{}",
            self.name,
            self.exit_status,
            self.stderr
        );
        Ok(self)
    }

    #[track_caller]
    pub(crate) fn ensure_exit_code(&self, code: i32) -> eyre::Result<&Self> {
        ensure!(
            self.exit_status == format!("exit code: {code}"),
            "{} failed with exit status {}. stderr:\n{}",
            self.name,
            self.exit_status,
            self.stderr
        );
        Ok(self)
    }

    #[track_caller]
    pub(crate) fn ensure_stdout<P>(&self, predicate: P) -> eyre::Result<&Self>
    where
        P: FnOnce(&str) -> bool,
    {
        ensure!(
            predicate(&self.stdout),
            "{} stdout did not satisfy the expected condition. stdout:\n{}",
            self.name,
            self.stdout
        );
        Ok(self)
    }

    #[track_caller]
    pub(crate) fn ensure_stderr<P>(&self, predicate: P) -> eyre::Result<&Self>
    where
        P: FnOnce(&str) -> bool,
    {
        ensure!(
            predicate(&self.stderr),
            "{} stderr did not satisfy the expected condition. stderr:\n{}",
            self.name,
            self.stderr
        );
        Ok(self)
    }

    #[track_caller]
    pub(crate) fn deserialize_stdout_as_jsonl<T>(&self) -> eyre::Result<Vec<T>>
    where
        T: for<'de> Deserialize<'de>,
    {
        self.stdout
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<Vec<_>, _>>()
            .wrap_err_with(|| {
                format!(
                    "{} stdout is not valid JSONL. stdout:\n{}",
                    self.name, self.stdout
                )
            })
    }
}

pub(crate) fn contains_line_eq(target: &str) -> impl Fn(&str) -> bool {
    move |s| s.lines().any(|line| line == target)
}

pub(crate) fn contains_line_starting_with(prefix: &str) -> impl Fn(&str) -> bool {
    move |s| s.lines().any(|line| line.starts_with(prefix))
}

pub(crate) fn not_contains_line_starting_with(prefix: &str) -> impl Fn(&str) -> bool {
    move |s| s.lines().all(|line| !line.starts_with(prefix))
}

const ERROR_PREFIX: &str = "error: ";
const WARNING_PREFIX: &str = "warning: ";

pub(crate) fn contains_error_line(stderr: &str) -> bool {
    contains_line_starting_with(ERROR_PREFIX)(stderr)
}

pub(crate) fn not_contains_error_line(stderr: &str) -> bool {
    not_contains_line_starting_with(ERROR_PREFIX)(stderr)
}

pub(crate) fn contains_warning_line(stderr: &str) -> bool {
    contains_line_starting_with(WARNING_PREFIX)(stderr)
}

pub(crate) fn not_contains_warning_line(stderr: &str) -> bool {
    not_contains_line_starting_with(WARNING_PREFIX)(stderr)
}

#[derive(Debug, Default)]
pub(crate) struct ReportContext {
    exec_results: Arc<Mutex<Vec<Arc<ExecResult>>>>,
}

impl ReportContext {
    pub(crate) fn push_exec_result(&self, exec_result: ExecResult) -> Arc<ExecResult> {
        let result = Arc::new(exec_result);
        self.exec_results.lock().unwrap().push(Arc::clone(&result));
        result
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct RunReport {
    pub(crate) id: RunId,
    pub(crate) kind: RunKind,
    pub(crate) outcome: RunOutcome,
    pub(crate) exec_results: Vec<Arc<ExecResult>>,
}

impl RunReport {
    pub(crate) fn capture<F>(id: RunId, kind: RunKind, f: F) -> (eyre::Result<()>, Self)
    where
        F: FnOnce(&ReportContext) -> eyre::Result<()> + UnwindSafe,
    {
        let cx = ReportContext::default();
        let res = run_with_panic_capture(|| f(&cx));
        let exec_results = mem::take(&mut *cx.exec_results.lock().unwrap());
        let report = Self {
            id,
            kind,
            outcome: (&res).into(),
            exec_results,
        };
        (res, report)
    }

    pub(crate) fn is_success(&self) -> bool {
        self.outcome.is_success()
    }

    pub(crate) fn print_summary(&self) {
        eprintln!("Run Summary:");
        eprintln!("  Run ID: {}", self.id);
        eprintln!("  Run Kind: {}", self.kind);

        for (i, res) in self.exec_results.iter().enumerate() {
            eprintln!();
            eprintln!("  Exec #{i}: {}", res.name);
            eprintln!("    Timestamp: {}", res.timestamp);
            eprintln!("    Caller: {}", res.caller);
            eprintln!("    Arguments: {:?}", res.arguments);
            eprintln!("    Exit Status: {}", res.exit_status);
            eprintln!("    Stdout: ({} bytes)", res.stdout.len());
            if !res.stdout.is_empty() {
                for line in res.stdout.lines() {
                    eprintln!("      {line}");
                }
            }
            eprintln!("    Stderr: ({} bytes)", res.stderr.len());
            if !res.stderr.is_empty() {
                for line in res.stderr.lines() {
                    eprintln!("      {line}");
                }
            }
        }

        match &self.outcome {
            RunOutcome::Success => eprintln!("  Result: Success"),
            RunOutcome::Failure { error } => {
                eprintln!("  Result: Failure");
                eprintln!("  Error:");
                for line in error.lines().skip_while(|line| line.trim().is_empty()) {
                    eprintln!("    {line}");
                }
            }
        }
    }
}

#[derive(Debug)]
struct CapturedPanic {
    thread: Thread,
    message: String,
    location: Option<String>,
}

impl CapturedPanic {
    fn from_panic_info(thread: Thread, panic_info: &PanicHookInfo<'_>) -> Self {
        let message = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            (*s).to_owned()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic payload".to_owned()
        };

        let location = panic_info.location().map(ToString::to_string);

        Self {
            thread,
            message,
            location,
        }
    }
}

impl Display for CapturedPanic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let thread_name = self.thread.name().unwrap_or("<unnamed>");
        let thread_id = self.thread.id();
        let loc = self.location.as_deref().unwrap_or("<unknown location>");
        let message = &self.message;
        writeln!(f, "thread {thread_name}({thread_id:?}) panicked at: {loc}")?;
        write!(f, "{message}")?;
        Ok(())
    }
}

static PANIC_SLOTS: LazyLock<Mutex<HashMap<ThreadId, Option<CapturedPanic>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(crate) fn init_panic_hook() {
    let prev_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        capture_panic(panic_info);
        prev_hook(panic_info);
    }));
}

fn capture_panic(panic_info: &PanicHookInfo<'_>) {
    let current_thread = thread::current();
    let thread_id = current_thread.id();
    let captured = CapturedPanic::from_panic_info(current_thread, panic_info);

    if let Some(slot) = PANIC_SLOTS.lock().unwrap().get_mut(&thread_id) {
        // Always overwrite with the latest panic.
        // If an inner `catch_unwind` triggers and handles a panic, that panic information
        // becomes obsolete. Overwriting ensures we capture the final, unhandled panic
        // that actually escapes the `run_with_panic_capture` scope.
        *slot = Some(captured);
    }
}

fn enter_panic_capture(thread_id: ThreadId) {
    *PANIC_SLOTS.lock().unwrap().entry(thread_id).or_default() = None;
}

fn take_captured_panic(thread_id: ThreadId) -> Option<CapturedPanic> {
    PANIC_SLOTS.lock().unwrap().remove(&thread_id).flatten()
}

fn run_with_panic_capture<F>(f: F) -> eyre::Result<()>
where
    F: FnOnce() -> eyre::Result<()> + UnwindSafe,
{
    let thread_id = thread::current().id();
    enter_panic_capture(thread_id);
    let res = panic::catch_unwind(f);
    let captured = take_captured_panic(thread_id);

    match res {
        Ok(res) => res,
        Err(panic_info) => {
            if let Some(captured) = captured {
                Err(eyre::eyre!("{captured}"))
            } else {
                let panic_payload = if let Some(s) = panic_info.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_info.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "<unknown panic payload>".to_string()
                };
                let thread = thread::current();
                let thread_name = thread.name().unwrap_or("<unnamed>");
                let thread_id = thread.id();
                Err(eyre::eyre!(
                    "thread {thread_name}({thread_id:?}) panicked at <unknown location>:\n{panic_payload}"
                ))
            }
        }
    }
}
