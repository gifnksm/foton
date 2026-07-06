use cargo_metadata::camino::Utf8PathBuf;
use color_eyre::eyre;
use serde::{Deserialize, Serialize};

use crate::{report::ReportContext, scenario::context::ScenarioContext};

mod cases;
mod context;
mod model;

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    clap::ValueEnum,
    derive_more::Display,
    Serialize,
    Deserialize,
    strum::EnumIter,
    strum::IntoStaticStr,
)]
#[display(rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Scenario {
    SanityCheck,
    InstallActivationLifecycle,
    UpdateLifecycle,
    ManifestValidation,
    InstallFailureCleanup,
}

#[derive(Debug)]
pub(crate) struct ScenarioParameters {
    pub(crate) foton_exe: Utf8PathBuf,
    pub(crate) fixture_dir: Utf8PathBuf,
    pub(crate) output_dir: Utf8PathBuf,
}

pub(crate) fn run(
    cx: &ReportContext,
    scenario: Scenario,
    params: &ScenarioParameters,
) -> eyre::Result<()> {
    let cx = ScenarioContext::new(cx, params);
    cases::run(&cx, scenario)
}
