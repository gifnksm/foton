use color_eyre::eyre::{self, ensure};

use crate::scenario::ScenarioContext;

const PKG_NAME: &str = "hackgen";

pub(super) fn run(cx: &ScenarioContext<'_>) -> eyre::Result<()> {
    let managed_packages = cx.list_packages()?;
    ensure!(managed_packages.is_empty());
    let installed_fonts = cx.list_package_fonts()?;
    ensure!(installed_fonts.is_empty());

    cx.exec_foton(|cmd| {
        cmd.args(["install", "--no-confirm", "--exit-on-lock", PKG_NAME]);
    })?
    .ensure_success()?;

    let managed_packages = cx.list_packages()?;
    ensure!(managed_packages.len() == 1);
    managed_packages[0]
        .ensure_name(|name| name == PKG_NAME)?
        .ensure_installation_state(|state| state.is_installed())?
        .ensure_activation_state(|state| state.is_active())?;
    let installed_pkg_id = &managed_packages[0].pkg_id;

    let installed_fonts = cx.list_package_fonts()?;
    ensure!(!installed_fonts.is_empty());
    ensure!(
        installed_fonts
            .iter()
            .all(|font| font.location.pkg_id.as_deref() == Some(installed_pkg_id))
    );

    cx.exec_foton(|cmd| {
        cmd.args(["uninstall", "--no-confirm", "--exit-on-lock", PKG_NAME]);
    })?
    .ensure_success()?;

    let managed_packages = cx.list_packages()?;
    ensure!(managed_packages.is_empty());
    let installed_fonts = cx.list_package_fonts()?;
    ensure!(installed_fonts.is_empty());

    Ok(())
}
