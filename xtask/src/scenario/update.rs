use color_eyre::eyre;

use crate::{report::ExecResult, scenario::ScenarioParameters};

const PKG_SPEC_OLD: &str = "hackgen@2.9.1";

pub(super) fn run(
    params: &ScenarioParameters,
    exec_results: &mut Vec<ExecResult>,
) -> eyre::Result<()> {
    super::exec_foton(params, exec_results, |cmd| {
        cmd.args(["list"]);
    })?
    .ensure_success()?
    .ensure_stdout(str::is_empty)?;

    super::exec_foton(params, exec_results, |cmd| {
        cmd.args(["install", PKG_SPEC_OLD, "--no-confirm"]);
    })?
    .ensure_success()?;

    super::exec_foton(params, exec_results, |cmd| {
        cmd.args(["list"]);
    })?
    .ensure_success()?
    .ensure_stdout(|stdout| stdout.lines().any(|line| line.starts_with("hackgen@2.9.1")))?;

    super::exec_foton(params, exec_results, |cmd| {
        cmd.args(["update", "--no-confirm"]);
    })?
    .ensure_success()?;

    let res = super::exec_foton(params, exec_results, |cmd| {
        cmd.args(["list"]);
    })?
    .ensure_success()?;

    let mut active_version = None;
    let mut inactive_version = None;
    for line in res.stdout.lines() {
        let Some(version_state) = line.strip_prefix("hackgen@") else {
            continue;
        };
        if let Some(version) = version_state.strip_suffix(" (active)") {
            assert!(active_version.is_none());
            active_version = Some(version.to_owned());
        }
        if let Some(version) = version_state.strip_suffix(" (inactive)") {
            assert!(inactive_version.is_none());
            inactive_version = Some(version.to_owned());
        }
    }
    let active_version = active_version.unwrap();
    let inactive_version = inactive_version.unwrap();
    assert_eq!(inactive_version, "2.9.1");

    super::exec_foton(params, exec_results, |cmd| {
        cmd.args(["uninstall", "--no-confirm"]);
        cmd.args([
            format!("hackgen@{active_version}"),
            format!("hackgen@{inactive_version}"),
        ]);
    })?
    .ensure_success()?;

    Ok(())
}
