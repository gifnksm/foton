use std::{
    fmt::{self, Display},
    process,
};

use chrono::{DateTime, Utc};
use color_eyre::eyre;
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
    pub(crate) success: bool,
    pub(crate) exit_status: String,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
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

        for output in &self.exec_results {
            eprintln!();
            if !output.stdout.is_empty() {
                eprintln!("---- {} stdout ----", output.name);
                eprintln!("{}", output.stdout);
            }
            if !output.stderr.is_empty() {
                eprintln!("---- {} stderr ----", output.name);
                eprintln!("{}", output.stderr);
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
