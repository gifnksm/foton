use std::{
    fmt::{self, Display},
    process,
};

use chrono::{DateTime, Utc};
use color_eyre::eyre::{self, ensure};
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
    Failure { error: String, sources: Vec<String> },
}

impl From<&eyre::Result<()>> for RunOutcome {
    fn from(value: &eyre::Result<()>) -> Self {
        match value {
            Ok(()) => RunOutcome::Success,
            Err(err) => RunOutcome::Failure {
                error: err.to_string(),
                sources: err.chain().skip(1).map(ToString::to_string).collect(),
            },
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct ExecResult {
    pub(crate) name: String,
    pub(crate) arguments: Vec<String>,
    pub(crate) success: bool,
    pub(crate) exit_status: String,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

impl ExecResult {
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
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct RunReport {
    pub(crate) id: RunId,
    pub(crate) kind: RunKind,
    pub(crate) outcome: RunOutcome,
    pub(crate) exec_results: Vec<ExecResult>,
}

impl RunReport {
    pub(crate) fn capture<F>(id: RunId, kind: RunKind, f: F) -> (eyre::Result<()>, Self)
    where
        F: FnOnce(&mut Vec<ExecResult>) -> eyre::Result<()>,
    {
        let mut exec_results = vec![];
        let res = f(&mut exec_results);
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
            RunOutcome::Failure { error, sources } => {
                eprintln!("  Result: Failure");
                eprintln!("  Error: {error}");
                for source in sources {
                    eprintln!("    caused by: {source}");
                }
            }
        }
    }
}
