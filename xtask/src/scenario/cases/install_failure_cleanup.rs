use color_eyre::eyre::{self, ensure};

use crate::{report, scenario::ScenarioContext, util::fs};

const PKG_NAME: &str = "tom-thumb";

pub(super) fn run(cx: &ScenarioContext<'_>) -> eyre::Result<()> {
    cx.install_fixture_config("foton-registry")?;
    cx.ensure_package_not_managed(PKG_NAME)?;
    cx.ensure_package_has_no_active_fonts(PKG_NAME)?;

    let fixture_dir = cx.params().fixture_dir.join("install_failure_cleanup");
    let manifest_path = fixture_dir.join("manifest.toml");
    ensure!(manifest_path.exists());

    let content = fs::read_to_string("manifest.toml", &manifest_path)?;
    ensure!(report::contains_line_eq(&format!(r#"name = "{PKG_NAME}""#))(&content));

    cx.exec_foton_with_args([
        "install",
        "--warnings-as-errors",
        "--manifest",
        manifest_path.as_str(),
    ])?
    .ensure_exit_code(1)?
    .ensure_stdout(str::is_empty)?
    .ensure_stderr(report::contains_error_line)?
    .ensure_stderr(|stderr| stderr.contains("failed to install"))?;

    cx.ensure_package_not_managed(PKG_NAME)?;
    cx.ensure_package_has_no_active_fonts(PKG_NAME)?;

    // ensure that subsequent installation from registry succeeds, and that the package is installed and active
    cx.exec_foton_with_args(["install", PKG_NAME])?
        .ensure_success()?;
    cx.ensure_package_managed(PKG_NAME)?
        .ensure_installation_state(|state| state.is_installed())?
        .ensure_activation_state(|state| state.is_active())?;
    cx.ensure_package_has_active_fonts(PKG_NAME)?;

    cx.uninstall_package(PKG_NAME)?;

    Ok(())
}
