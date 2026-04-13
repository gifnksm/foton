use std::process::Command;

use color_eyre::eyre::{self, WrapErr as _, ensure};

use crate::{fs_util, scenario::RunArgs};

pub(super) fn run(args: &RunArgs) -> eyre::Result<()> {
    let stdout_path = args.output_dir.join("stdout.txt");
    let stderr_path = args.output_dir.join("stderr.txt");
    let exitcode_path = args.output_dir.join("exitcode.txt");

    let stdout_file = fs_util::create_file("foton stdout", &stdout_path)?;
    let stderr_file = fs_util::create_file("foton stderr", &stderr_path)?;

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

    fs_util::write(
        "foton exitcode",
        &exitcode_path,
        format!("{}", status.code().unwrap_or(255)),
    )?;

    let stdout = fs_util::read_to_string("foton stdout", &stdout_path)?;
    let stderr = fs_util::read_to_string("foton stderr", &stderr_path)?;

    ensure!(status.success(), "foton exitcode is not 0: {}", status);
    ensure!(
        stdout.contains("Usage:"),
        "foton stdout does not contain `Usage:`"
    );
    ensure!(stderr.is_empty(), "foton stderr is not empty");

    Ok(())
}
