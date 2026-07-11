use color_eyre::eyre::{self, ensure};

use crate::{scenario::ScenarioContext, util::env as env_util};

const PKG_NAMES: [&str; 2] = ["fira-code", "jetbrains-mono"];

pub(super) fn run(cx: &ScenarioContext<'_>) -> eyre::Result<()> {
    let config_path = env_util::foton_config_path()?;
    ensure!(
        !config_path.exists(),
        "default-registry-install requires no existing foton config file: {config_path}"
    );

    for pkg_name in PKG_NAMES {
        cx.ensure_package_not_managed(pkg_name)?;
        cx.ensure_package_has_no_active_fonts(pkg_name)?;
    }

    // Run two separate installs so we cover both registry fetch paths:
    // the first install populates the cache from an absent registry checkout,
    // and the second install reuses and updates the existing cached checkout.
    for pkg_name in PKG_NAMES {
        cx.install_package(pkg_name)?;
    }

    for pkg_name in PKG_NAMES {
        cx.uninstall_package(pkg_name)?;
    }

    Ok(())
}
