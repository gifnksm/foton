use std::{
    fs::{self, File},
    process::Command,
};

use color_eyre::eyre::{self, WrapErr as _, ensure};

use crate::scenario::RunArgs;

pub(super) fn run(args: &RunArgs) -> eyre::Result<()> {
    let stdout_path = args.output_dir.join("stdout.txt");
    let stderr_path = args.output_dir.join("stderr.txt");
    let exitcode_path = args.output_dir.join("exitcode.txt");

    let stdout_file = File::create(&stdout_path).wrap_err_with(|| {
        format!(
            "failed to create foton stdout file: {}",
            stdout_path.display()
        )
    })?;
    let stderr_file = File::create(&stderr_path).wrap_err_with(|| {
        format!(
            "failed to create foton stderr file: {}",
            stderr_path.display()
        )
    })?;

    let status = Command::new(&args.foton_exe)
        .arg("--help")
        .stdout(stdout_file)
        .stderr(stderr_file)
        .status()
        .wrap_err_with(|| {
            format!(
                "failed to execute foton command: {}",
                args.foton_exe.display()
            )
        })?;

    fs::write(&exitcode_path, format!("{}", status.code().unwrap_or(255))).wrap_err_with(|| {
        format!(
            "failed to write foton exitcode: {}",
            exitcode_path.display()
        )
    })?;

    let stdout = fs::read_to_string(&stdout_path).wrap_err_with(|| {
        format!(
            "failed to read foton stdout file: {}",
            stdout_path.display()
        )
    })?;

    ensure!(status.success(), "foton exitcode is not 0: {}", status);
    ensure!(
        stdout.contains("Usage:"),
        "foton stdout does not contain `Usage:`"
    );

    Ok(())
}
