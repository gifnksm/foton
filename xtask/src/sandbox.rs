use cargo_metadata::camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};

use crate::{
    report::{RunId, RunKind},
    scenario::Scenario,
};

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct SandboxBootstrapConfig {
    pub(crate) execution_environment: String,
    pub(crate) foton_exe: Utf8PathBuf,
    pub(crate) xtask_exe: Utf8PathBuf,
    pub(crate) fixture_dir: Utf8PathBuf,
    pub(crate) output_dir: Utf8PathBuf,
    pub(crate) complete_stamp: Utf8PathBuf,
    pub(crate) run_id: RunId,
    pub(crate) action: SandboxAction,
    pub(crate) envs: Vec<(String, String)>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SandboxAction {
    Noop,
    RunTests {
        test_exes: Vec<Utf8PathBuf>,
        report_json: Utf8PathBuf,
    },
    RunScenario {
        scenario: Scenario,
        report_json: Utf8PathBuf,
    },
}

impl SandboxAction {
    pub(crate) fn to_kind(&self) -> RunKind {
        match self {
            Self::Noop => RunKind::Noop,
            Self::RunTests { .. } => RunKind::Test,
            Self::RunScenario { scenario, .. } => RunKind::Scenario(*scenario),
        }
    }
}
