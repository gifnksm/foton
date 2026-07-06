use cargo_metadata::camino::Utf8PathBuf;
use color_eyre::eyre;

use crate::{report, scenario::ScenarioContext};

pub(super) fn run(cx: &ScenarioContext<'_>) -> eyre::Result<()> {
    let fixture_dir = cx.params().fixture_dir.join("manifest_validation");

    run_validation(
        cx,
        ManifestValidationCase {
            manifest_path: fixture_dir.join("manifest_without_warning.toml"),
            has_source_warnings: false,
            has_license_warnings: false,
        },
    )?;
    run_validation(
        cx,
        ManifestValidationCase {
            manifest_path: fixture_dir.join("manifest_with_source_warning.toml"),
            has_source_warnings: true,
            has_license_warnings: false,
        },
    )?;
    run_validation(
        cx,
        ManifestValidationCase {
            manifest_path: fixture_dir.join("manifest_with_license_warning.toml"),
            has_source_warnings: false,
            has_license_warnings: true,
        },
    )?;
    run_validation(
        cx,
        ManifestValidationCase {
            manifest_path: fixture_dir.join("manifest_with_both_warning.toml"),
            has_source_warnings: true,
            has_license_warnings: true,
        },
    )?;

    Ok(())
}

#[derive(Debug)]
struct ManifestValidationCase {
    manifest_path: Utf8PathBuf,
    has_source_warnings: bool,
    has_license_warnings: bool,
}

const EXIT_SUCCESS: i32 = 0;
const EXIT_FAILURE: i32 = 1;

fn run_validation(cx: &ScenarioContext<'_>, case: ManifestValidationCase) -> eyre::Result<()> {
    let ManifestValidationCase {
        manifest_path,
        has_source_warnings,
        has_license_warnings,
    } = case;

    eyre::ensure!(manifest_path.exists());
    let has_any_warnings = has_source_warnings || has_license_warnings;

    cx.exec_foton_with_args(["manifest", "check", manifest_path.as_str()])?
        .ensure_success()?
        .ensure_stdout(str::is_empty)?
        .ensure_stderr(if has_any_warnings {
            report::contains_warning_line
        } else {
            report::not_contains_warning_line
        })?
        .ensure_stderr(report::not_contains_error_line)?;

    cx.exec_foton_with_args([
        "manifest",
        "check",
        "--warnings-as-errors",
        manifest_path.as_str(),
    ])?
    .ensure_exit_code(if has_any_warnings {
        EXIT_FAILURE
    } else {
        EXIT_SUCCESS
    })?
    .ensure_stdout(str::is_empty)?
    .ensure_stderr(report::not_contains_warning_line)?
    .ensure_stderr(if has_any_warnings {
        report::contains_error_line
    } else {
        report::not_contains_error_line
    })?;

    cx.exec_foton_with_args([
        "manifest",
        "check",
        "--warnings-as-errors",
        "--no-source-checks",
        manifest_path.as_str(),
    ])?
    .ensure_exit_code(if has_license_warnings {
        EXIT_FAILURE
    } else {
        EXIT_SUCCESS
    })?
    .ensure_stdout(str::is_empty)?
    .ensure_stderr(report::not_contains_warning_line)?
    .ensure_stderr(if has_license_warnings {
        report::contains_error_line
    } else {
        report::not_contains_error_line
    })?;

    Ok(())
}
