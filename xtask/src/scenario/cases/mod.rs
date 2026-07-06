use color_eyre::eyre;

use crate::scenario::{Scenario, ScenarioContext};

mod install_activation_lifecycle;
mod install_failure_cleanup;
mod manifest_validation;
mod sanity_check;
mod update_lifecycle;

pub(in crate::scenario) fn run(cx: &ScenarioContext<'_>, scenario: Scenario) -> eyre::Result<()> {
    match scenario {
        Scenario::SanityCheck => sanity_check::run(cx),
        Scenario::InstallActivationLifecycle => install_activation_lifecycle::run(cx),
        Scenario::UpdateLifecycle => update_lifecycle::run(cx),
        Scenario::ManifestValidation => manifest_validation::run(cx),
        Scenario::InstallFailureCleanup => install_failure_cleanup::run(cx),
    }
}
