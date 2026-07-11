use color_eyre::eyre::{self, ensure};

use crate::{report, scenario::ScenarioContext};

const PKG_NAME: &str = "tom-thumb";
const OLD_PKG_ID: &str = "tom-thumb@2023.02.25.00";
const NEW_PKG_ID: &str = "tom-thumb@2023.02.25.01";

pub(super) fn run(cx: &ScenarioContext<'_>) -> eyre::Result<()> {
    cx.install_fixture_config("foton-registry-update")?;

    cx.ensure_package_not_managed(PKG_NAME)?;
    cx.ensure_package_has_no_active_fonts(PKG_NAME)?;

    cx.exec_foton_with_args(["install", OLD_PKG_ID])?
        .ensure_success()?;

    cx.ensure_package_managed(OLD_PKG_ID)?
        .ensure_installation_state(|state| state.is_installed())?
        .ensure_activation_state(|state| state.is_active())?;
    let active_fonts = cx.ensure_package_has_active_fonts(OLD_PKG_ID)?;
    ensure!(
        active_fonts
            .iter()
            .all(|font| font.location.is_pkg_font(OLD_PKG_ID))
    );

    // ensure that second update attempt is no-op, and that the package remains installed and active
    for _ in 0..2 {
        cx.exec_foton_with_args(["update"])?.ensure_success()?;

        let old_pkg = cx.ensure_package_managed(OLD_PKG_ID)?;
        let new_pkg = cx.ensure_package_managed(NEW_PKG_ID)?;
        old_pkg
            .ensure_installation_state(|state| state.is_installed())?
            .ensure_activation_state(|state| state.is_inactive())?;
        new_pkg
            .ensure_installation_state(|state| state.is_installed())?
            .ensure_activation_state(|state| state.is_active())?;

        let active_fonts = cx.query_package_active_fonts(PKG_NAME)?;
        ensure!(!active_fonts.is_empty());
        ensure!(
            active_fonts
                .iter()
                .all(|font| font.location.is_pkg_font(&new_pkg.pkg_id))
        );

        cx.exec_foton_with_args(["info", PKG_NAME])?
            .ensure_success()?
            .ensure_stdout(report::contains_line_starting_with(&format!(
                "Package ID: {OLD_PKG_ID}"
            )))?
            .ensure_stdout(report::contains_line_starting_with(&format!(
                "Package ID: {}",
                new_pkg.pkg_id
            )))?;
    }

    cx.uninstall_package(OLD_PKG_ID)?;
    cx.uninstall_package(NEW_PKG_ID)?;

    Ok(())
}
