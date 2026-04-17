use std::{
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

use cargo_metadata::camino::Utf8Path;
use color_eyre::eyre::{self, WrapErr as _};

use crate::{fs_util, report::ExecResult};

pub(crate) fn exec_command<S>(
    name: S,
    output_dir: &Utf8Path,
    cmd: &mut Command,
) -> eyre::Result<ExecResult>
where
    S: Into<String>,
{
    static EXEC_COUNTER: AtomicUsize = AtomicUsize::new(0);

    let name = name.into();

    let id = EXEC_COUNTER.fetch_add(1, Ordering::Relaxed);
    let file_prefix = format_args!("{id}.{name}");

    let stdout_path = output_dir.join(format!("{file_prefix}.stdout.txt"));
    let stderr_path = output_dir.join(format!("{file_prefix}.stderr.txt"));
    let status_path = output_dir.join(format!("{file_prefix}.status.txt"));

    let stdout_file = fs_util::create_file(format_args!("{name} stdout"), &stdout_path)?;
    let stderr_file = fs_util::create_file(format_args!("{name} stderr"), &stderr_path)?;

    let status = cmd
        .stdout(stdout_file)
        .stderr(stderr_file)
        .status()
        .wrap_err_with(|| format!("failed to execute {name} (exec #{id})"))?;

    let stdout = fs_util::read_to_string(format_args!("{name} stdout"), &stdout_path)?;
    let stderr = fs_util::read_to_string(format_args!("{name} stderr"), &stderr_path)?;

    fs_util::write(
        format_args!("{name} status"),
        &status_path,
        status.to_string(),
    )?;

    Ok(ExecResult {
        name,
        success: status.success(),
        exit_status: status.to_string(),
        stdout,
        stderr,
    })
}
