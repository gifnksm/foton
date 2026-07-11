use color_eyre::eyre::{self, ensure};

use crate::{scenario::ScenarioContext, util::env as env_util};

const PKG_NAME: &str = "fira-code";

pub(super) fn run(cx: &ScenarioContext<'_>) -> eyre::Result<()> {
    let config_path = env_util::foton_config_path()?;
    ensure!(
        !config_path.exists(),
        "default-registry-install requires no existing FOTON config file: {config_path}"
    );

    cx.ensure_package_not_managed(PKG_NAME)?;
    cx.ensure_package_has_no_active_fonts(PKG_NAME)?;

    cx.install_package(PKG_NAME)?;
    cx.uninstall_package(PKG_NAME)?;
    Ok(())
}
