use std::{
    collections::HashMap,
    fmt::{self, Display},
    mem,
    panic::{self, PanicHookInfo, UnwindSafe},
    process::{self, Command},
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicU32, Ordering},
    },
    thread::{self, Thread, ThreadId},
    time::Duration,
};

use cargo_metadata::camino::{Utf8Path, Utf8PathBuf};
use chrono::{DateTime, Utc};
use color_eyre::eyre::{self, WrapErr as _, ensure};
use serde::{Deserialize, Serialize};

use crate::{
    scenario::Scenario,
    util::{fs as fs_util, process as process_util},
};

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
    pub(crate) duration: Duration,
    pub(crate) caller: String,
    pub(crate) program: String,
    pub(crate) arguments: Vec<String>,
    pub(crate) success: bool,
    pub(crate) exit_status: String,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

impl Display for ExecResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            timestamp,
            duration,
            caller,
            program,
            arguments,
            success,
            exit_status,
            stdout,
            stderr,
        } = self;

        writeln!(f, "Timestamp: {timestamp}")?;
        writeln!(f, "Duration: {duration:?}")?;
        writeln!(f, "Caller: {caller}")?;
        writeln!(f, "Program: {program}")?;
        writeln!(f, "Arguments: {arguments:?}")?;
        writeln!(f, "Success: {success}")?;
        writeln!(f, "Exit Status: {exit_status}")?;
        writeln!(f, "Stdout: ({} bytes)", stdout.len())?;
        if !stdout.is_empty() {
            for line in stdout.lines() {
                writeln!(f, "  {line}")?;
            }
        }
        writeln!(f, "Stderr: ({} bytes)", stderr.len())?;
        if !stderr.is_empty() {
            for line in stderr.lines() {
                writeln!(f, "  {line}")?;
            }
        }
        Ok(())
    }
}

impl ExecResult {
    #[track_caller]
    pub(crate) fn ensure_success(&self) -> eyre::Result<&Self> {
        ensure!(
            self.success,
            "{} failed with exit status {}. stderr:\n{}",
            self.program,
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
            self.program,
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
            self.program,
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
            self.program,
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
                    self.program, self.stdout
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

#[derive(Debug)]
pub(crate) struct ReportContext {
    report_seq: u32,
    output_dir: Utf8PathBuf,
    exec_seq: AtomicU32,
    exec_results: Arc<Mutex<Vec<Arc<ExecResult>>>>,
}

impl ReportContext {
    fn new(output_base_dir: &Utf8Path) -> Self {
        static REPORT_SEQ: AtomicU32 = AtomicU32::new(0);
        let report_seq = REPORT_SEQ.fetch_add(1, Ordering::Relaxed);
        let output_dir = output_base_dir.join(format!("report.{report_seq}"));
        Self {
            report_seq,
            output_dir,
            exec_seq: AtomicU32::new(0),
            exec_results: Arc::new(Mutex::new(Vec::new())),
        }
    }

    #[track_caller]
    pub(crate) fn exec_command(&self, cmd: &mut Command) -> eyre::Result<Arc<ExecResult>> {
        fs_util::create_dir_all(
            format_args!("report #{} output directory", self.report_seq),
            &self.output_dir,
        )?;

        let exec_seq = self.exec_seq.fetch_add(1, Ordering::Relaxed);
        let exec_result = process_util::exec_command(exec_seq, &self.output_dir, cmd)?;
        let exec_result = Arc::new(exec_result);
        self.exec_results
            .lock()
            .unwrap()
            .push(Arc::clone(&exec_result));
        Ok(exec_result)
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct RunReport {
    pub(crate) id: RunId,
    pub(crate) kind: RunKind,
    pub(crate) outcome: RunOutcome,
    pub(crate) exec_results: Vec<Arc<ExecResult>>,
}

impl Display for RunReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            id,
            kind,
            outcome,
            exec_results,
        } = self;
        writeln!(f, "Run ID: {id}")?;
        writeln!(f, "Run Kind: {kind}")?;
        for (i, res) in exec_results.iter().enumerate() {
            writeln!(f, "Exec Result #{i}:")?;
            for line in res.to_string().lines() {
                writeln!(f, "  {line}")?;
            }
        }
        match outcome {
            RunOutcome::Success => writeln!(f, "Result: Success")?,
            RunOutcome::Failure { error } => {
                writeln!(f, "Result: Failure")?;
                writeln!(f, "Error:")?;
                for line in error.lines().skip_while(|line| line.trim().is_empty()) {
                    writeln!(f, "  {line}")?;
                }
            }
        }
        Ok(())
    }
}

impl RunReport {
    pub(crate) fn capture<F>(
        id: RunId,
        kind: RunKind,
        output_base_dir: &Utf8Path,
        f: F,
    ) -> (eyre::Result<()>, Self)
    where
        F: FnOnce(&ReportContext) -> eyre::Result<()> + UnwindSafe,
    {
        let cx = ReportContext::new(output_base_dir);
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
