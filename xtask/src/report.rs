use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct ScenarioReport {
    pub(crate) outcome: ScenarioOutcome,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) enum ScenarioOutcome {
    Success,
    Failure { error: String, sources: Vec<String> },
}
