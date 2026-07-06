use color_eyre::eyre;

use crate::{report, scenario::ScenarioContext};

const PKG_NAME: &str = "hackgen";

pub(super) fn run(cx: &ScenarioContext<'_>) -> eyre::Result<()> {
    implicit_activation(cx)?;
    explicit_activation(cx)?;
    Ok(())
}

fn implicit_activation(cx: &ScenarioContext<'_>) -> eyre::Result<()> {
    cx.ensure_package_not_managed(PKG_NAME)?;
    cx.ensure_package_has_no_active_fonts(PKG_NAME)?;

    // ensure that second installation attempt is no-op, and that the package remains active
    for _ in 0..2 {
        cx.exec_foton_with_args(["install", PKG_NAME])?
            .ensure_success()?;
        cx.ensure_package_installed(PKG_NAME)?
            .ensure_activation_state(|state| state.is_active())?;
        cx.ensure_package_has_active_fonts(PKG_NAME)?;
    }

    cx.exec_foton_with_args(["info", PKG_NAME])?
        .ensure_success()?
        .ensure_stdout(report::contains_line_starting_with(&format!(
            "Package ID: {PKG_NAME}@"
        )))?;

    // ensure that second uninstallation attempt is no-op, and that the package remains uninstalled
    for _ in 0..2 {
        cx.exec_foton_with_args(["uninstall", PKG_NAME])?
            .ensure_success()?;
        cx.ensure_package_not_managed(PKG_NAME)?;
        cx.ensure_package_has_no_active_fonts(PKG_NAME)?;
    }

    Ok(())
}

fn explicit_activation(cx: &ScenarioContext<'_>) -> eyre::Result<()> {
    cx.ensure_package_not_managed(PKG_NAME)?;
    cx.ensure_package_has_no_active_fonts(PKG_NAME)?;

    // ensure that second installation attempt is no-op, and that the package remains installed and inactive
    for _ in 0..2 {
        cx.exec_foton_with_args(["install", "--no-activate", PKG_NAME])?
            .ensure_success()?;

        cx.ensure_package_installed(PKG_NAME)?
            .ensure_activation_state(|state| state.is_inactive())?;
        cx.ensure_package_has_no_active_fonts(PKG_NAME)?;
    }

    // ensure that second activation attempt is no-op, and the package remains active
    for _ in 0..2 {
        cx.exec_foton_with_args(["activate", PKG_NAME])?
            .ensure_success()?;

        cx.ensure_package_installed(PKG_NAME)?
            .ensure_activation_state(|state| state.is_active())?;
        cx.ensure_package_has_active_fonts(PKG_NAME)?;
    }

    cx.exec_foton_with_args(["info", PKG_NAME])?
        .ensure_success()?
        .ensure_stdout(report::contains_line_starting_with(&format!(
            "Package ID: {PKG_NAME}@"
        )))?;

    // ensure that second deactivation attempt is no-op, and the package remains inactive
    for _ in 0..2 {
        cx.exec_foton_with_args(["deactivate", PKG_NAME])?
            .ensure_success()?;

        cx.ensure_package_installed(PKG_NAME)?
            .ensure_activation_state(|state| state.is_inactive())?;
        cx.ensure_package_has_no_active_fonts(PKG_NAME)?;
    }

    // ensure that second uninstallation attempt is no-op, and that the package remains uninstalled
    for _ in 0..2 {
        cx.exec_foton_with_args(["uninstall", PKG_NAME])?
            .ensure_success()?;

        cx.ensure_package_not_managed(PKG_NAME)?;
        cx.ensure_package_has_no_active_fonts(PKG_NAME)?;
    }

    Ok(())
}
