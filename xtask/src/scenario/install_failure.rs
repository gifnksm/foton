use color_eyre::eyre::{self, ensure};

use crate::{report, scenario::ScenarioContext};

pub(super) fn run(cx: &ScenarioContext<'_>) -> eyre::Result<()> {
    let fixture_dir = cx.params.fixture_dir.join("install_failure");

    cx.exec_foton(|cmd| {
        cmd.args([
            "install",
            "--no-confirm",
            "--warnings-as-errors",
            "--exit-on-lock",
            "--manifest",
        ])
        .arg(fixture_dir.join("manifest.toml"));
    })?
    .ensure_exit_code(1)?
    .ensure_stdout(str::is_empty)?
    .ensure_stderr(report::contains_error_line)?
    .ensure_stderr(|stderr| {
        stderr
            .lines()
            .any(|line| line.contains("failed to install"))
    })?;

    let managed_packages = cx.list_packages()?;
    ensure!(managed_packages.is_empty());
    let installed_fonts = cx.list_package_fonts()?;
    ensure!(installed_fonts.is_empty());

    Ok(())
}
