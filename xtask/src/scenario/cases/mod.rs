use color_eyre::eyre;

use crate::scenario::{Scenario, ScenarioContext};

mod help_check;
mod install_failure;
mod install_uninstall;
mod manifest_check;
mod update;

pub(in crate::scenario) fn run(cx: &ScenarioContext<'_>, scenario: Scenario) -> eyre::Result<()> {
    match scenario {
        Scenario::HelpCheck => help_check::run(cx),
        Scenario::InstallUninstall => install_uninstall::run(cx),
        Scenario::Update => update::run(cx),
        Scenario::ManifestCheck => manifest_check::run(cx),
        Scenario::InstallFailure => install_failure::run(cx),
    }
}
