use color_eyre::eyre::{self, ensure};

use crate::{report::ExecResult, scenario::ScenarioParameters};

const PKG_NAME: &str = "hackgen";

pub(super) fn run(
    params: &ScenarioParameters,
    exec_results: &mut Vec<ExecResult>,
) -> eyre::Result<()> {
    let managed_packages = super::list_packages(params, exec_results)?;
    ensure!(managed_packages.is_empty());
    let installed_fonts = super::list_package_fonts(params, exec_results)?;
    ensure!(installed_fonts.is_empty());

    super::exec_foton(params, exec_results, |cmd| {
        cmd.args(["install", "--no-confirm", "--exit-on-lock", PKG_NAME]);
    })?
    .ensure_success()?;

    let managed_packages = super::list_packages(params, exec_results)?;
    ensure!(managed_packages.len() == 1);
    managed_packages[0]
        .ensure_name(|name| name == PKG_NAME)?
        .ensure_installation_state(|state| state.is_installed())?
        .ensure_activation_state(|state| state.is_active())?;
    let installed_pkg_id = &managed_packages[0].pkg_id;

    let installed_fonts = super::list_package_fonts(params, exec_results)?;
    ensure!(!installed_fonts.is_empty());
    ensure!(
        installed_fonts
            .iter()
            .all(|font| font.location.pkg_id.as_deref() == Some(installed_pkg_id))
    );

    super::exec_foton(params, exec_results, |cmd| {
        cmd.args(["uninstall", "--no-confirm", "--exit-on-lock", PKG_NAME]);
    })?
    .ensure_success()?;

    let managed_packages = super::list_packages(params, exec_results)?;
    ensure!(managed_packages.is_empty());
    let installed_fonts = super::list_package_fonts(params, exec_results)?;
    ensure!(installed_fonts.is_empty());

    Ok(())
}
