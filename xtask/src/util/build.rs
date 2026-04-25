use std::{
    io::BufReader,
    process::{Command, Stdio},
};

use cargo_metadata::{CrateType, Message, camino::Utf8PathBuf};
use color_eyre::eyre::{self, OptionExt as _, WrapErr as _, ensure};

use crate::util::env as env_util;

pub(crate) fn build_foton_exe() -> eyre::Result<Utf8PathBuf> {
    let cargo_bin = env_util::cargo_bin()?;

    let mut command = Command::new(cargo_bin)
        .args([
            "build",
            "-p",
            "foton",
            "--message-format=json-render-diagnostics",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .wrap_err("failed to spawn `cargo build -p foton`")?;

    let mut foton_exe = None;
    let reader = BufReader::new(command.stdout.take().unwrap());
    for message in Message::parse_stream(reader) {
        let message = message.wrap_err("failed to parse cargo message")?;
        if let Message::CompilerArtifact(artifact) = message
            && artifact.target.is_bin()
            && artifact.target.crate_types.contains(&CrateType::Bin)
            && artifact.target.name == "foton"
            && let Some(exe) = artifact.executable
        {
            foton_exe = Some(exe);
        }
    }

    let status = command.wait().wrap_err("failed to wait for cargo build")?;
    ensure!(status.success(), "cargo build failed with status {status}");

    let foton_exe = foton_exe.ok_or_eyre("failed to find foton executable in cargo output")?;
    Ok(foton_exe)
}

pub(crate) fn build_test_exes() -> eyre::Result<Vec<Utf8PathBuf>> {
    let cargo_bin = env_util::cargo_bin()?;

    let mut command = Command::new(cargo_bin)
        .args([
            "test",
            "--no-run",
            "-p",
            "foton",
            "--message-format=json-render-diagnostics",
        ])
        .env("FOTON_SANDBOX_TEST", "1")
        .stdout(Stdio::piped())
        .spawn()
        .wrap_err("failed to spawn `cargo test --no-run -p foton`")?;

    let mut test_exes = vec![];
    let reader = BufReader::new(command.stdout.take().unwrap());
    for message in Message::parse_stream(reader) {
        let message = message.wrap_err("failed to parse cargo message")?;
        if let Message::CompilerArtifact(artifact) = message
            && artifact.target.name == "foton"
            && let Some(exe) = artifact.executable
        {
            test_exes.push(exe);
        }
    }

    let status = command.wait().wrap_err("failed to wait for cargo test")?;
    ensure!(status.success(), "cargo test failed with status {status}");

    ensure!(
        !test_exes.is_empty(),
        "cargo test did not produce any test executables for foton"
    );
    Ok(test_exes)
}
