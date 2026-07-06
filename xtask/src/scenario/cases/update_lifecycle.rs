use color_eyre::eyre::{self, ensure};

use crate::{report, scenario::ScenarioContext};

const PKG_NAME: &str = "hackgen";
const OLD_PKG_VERSION: &str = "2.9.1";
const OLD_PKG_ID: &str = "hackgen@2.9.1";

pub(super) fn run(cx: &ScenarioContext<'_>) -> eyre::Result<()> {
    cx.ensure_package_not_managed(PKG_NAME)?;
    cx.ensure_package_has_no_active_fonts(PKG_NAME)?;

    cx.exec_foton_with_args(["install", OLD_PKG_ID])?
        .ensure_success()?;

    cx.ensure_package_installed(OLD_PKG_ID)?
        .ensure_activation_state(|state| state.is_active())?;
    let active_fonts = cx.ensure_package_has_active_fonts(OLD_PKG_ID)?;
    ensure!(
        active_fonts
            .iter()
            .all(|font| font.location.is_pkg_font(OLD_PKG_ID))
    );

    let mut new_pkg_version = None;

    // ensure that second update attempt is no-op, and that the package remains installed and active
    for _ in 0..2 {
        cx.exec_foton_with_args(["update"])?.ensure_success()?;

        let managed_packages = cx.query_managed_packages(PKG_NAME)?;
        ensure!(managed_packages.len() == 2);
        let (old_pkg, new_pkg) = if managed_packages[0].pkg_id == OLD_PKG_ID {
            (&managed_packages[0], &managed_packages[1])
        } else {
            ensure!(managed_packages[1].pkg_id == OLD_PKG_ID);
            (&managed_packages[1], &managed_packages[0])
        };
        old_pkg
            .ensure_installation_state(|state| state.is_installed())?
            .ensure_activation_state(|state| state.is_inactive())?;
        new_pkg
            .ensure_version(|version| version != OLD_PKG_VERSION)?
            .ensure_installation_state(|state| state.is_installed())?
            .ensure_activation_state(|state| state.is_active())?;
        if let Some(prev_version) = &new_pkg_version {
            ensure!(new_pkg.version() == prev_version);
        }
        new_pkg_version = Some(new_pkg.version().to_owned());

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

    let new_pkg_version = new_pkg_version.unwrap();
    cx.exec_foton_with_args([
        "uninstall",
        &format!("{PKG_NAME}@{OLD_PKG_VERSION}"),
        &format!("{PKG_NAME}@{new_pkg_version}"),
    ])?
    .ensure_success()?;

    Ok(())
}
