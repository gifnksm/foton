use color_eyre::eyre::{self, ensure};

use crate::scenario::ScenarioContext;

const PKG_NAME: &str = "hackgen";
const OLD_PKG_VERSION: &str = "2.9.1";
const OLD_PKG_ID: &str = "hackgen@2.9.1";

pub(super) fn run(cx: &ScenarioContext<'_>) -> eyre::Result<()> {
    let managed_packages = cx.list_packages()?;
    ensure!(managed_packages.is_empty());
    let installed_fonts = cx.list_package_fonts()?;
    ensure!(installed_fonts.is_empty());

    cx.exec_foton(|cmd| {
        cmd.args(["install", "--no-confirm", "--exit-on-lock", OLD_PKG_ID]);
    })?
    .ensure_success()?;

    let managed_packages = cx.list_packages()?;
    ensure!(managed_packages.len() == 1);
    managed_packages[0]
        .ensure_name(|name| name == PKG_NAME)?
        .ensure_version(|version| version == OLD_PKG_VERSION)?
        .ensure_installation_state(|state| state.is_installed())?
        .ensure_activation_state(|state| state.is_active())?;
    let installed_fonts = cx.list_package_fonts()?;
    ensure!(!installed_fonts.is_empty());
    ensure!(
        installed_fonts
            .iter()
            .all(|font| font.location.pkg_id.as_deref() == Some(OLD_PKG_ID))
    );

    cx.exec_foton(|cmd| {
        cmd.args(["update", "--no-confirm", "--exit-on-lock"]);
    })?
    .ensure_success()?;

    let managed_packages = cx.list_packages()?;
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

    let installed_fonts = cx.list_package_fonts()?;
    ensure!(!installed_fonts.is_empty());

    // DirectWrite can continue to report fonts from the old package
    // immediately after unregistration, so we do not require all
    // visible package fonts to belong to the newly active package.
    // Instead, we only check that at least one visible package font
    // belongs to the new package.
    ensure!(
        installed_fonts
            .iter()
            .any(|font| font.location.pkg_id.as_deref() == Some(&new_pkg.pkg_id))
    );

    cx.exec_foton(|cmd| {
        cmd.args(["uninstall", "--no-confirm", "--exit-on-lock"]);
        cmd.args([
            format!("{PKG_NAME}@{OLD_PKG_VERSION}"),
            format!("{PKG_NAME}@{}", new_pkg.version()),
        ]);
    })?
    .ensure_success()?;

    Ok(())
}
