use std::{fmt::Display, panic::Location, process::Command, time::Instant};

use cargo_metadata::camino::Utf8Path;
use chrono::Utc;
use color_eyre::eyre::{self, WrapErr as _};

use crate::{report::ExecResult, util::fs as fs_util};

#[track_caller]
pub(crate) fn exec_command<N>(
    id: N,
    output_dir: &Utf8Path,
    cmd: &mut Command,
) -> eyre::Result<ExecResult>
where
    N: Display,
{
    struct PrettyId<'a, 'b, N>(&'a Command, &'b N);
    impl<N> Display for PrettyId<'_, '_, N>
    where
        N: Display,
    {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{} (exec #{})", self.0.get_program().display(), self.1)
        }
    }

    let summary_path = output_dir.join(format!("{id}.summary.txt"));
    let stdout_path = output_dir.join(format!(".{id}.stdout.txt"));
    let stderr_path = output_dir.join(format!(".{id}.stderr.txt"));

    let stdout_file =
        fs_util::create_file(format_args!("{} stdout", PrettyId(cmd, &id)), &stdout_path)?;
    let stderr_file =
        fs_util::create_file(format_args!("{} stderr", PrettyId(cmd, &id)), &stderr_path)?;

    let timestamp = Utc::now();
    let start_time = Instant::now();

    let status = cmd
        .stdout(stdout_file)
        .stderr(stderr_file)
        .status()
        .wrap_err_with(|| format!("failed to execute {}", PrettyId(cmd, &id)))?;

    let duration = start_time.elapsed();

    let stdout =
        fs_util::read_to_string(format_args!("{} stdout", PrettyId(cmd, &id)), &stdout_path)?;
    let stderr =
        fs_util::read_to_string(format_args!("{} stderr", PrettyId(cmd, &id)), &stderr_path)?;

    let exec_result = ExecResult {
        timestamp,
        duration,
        caller: Location::caller().to_string(),
        program: cmd.get_program().display().to_string(),
        arguments: cmd
            .get_args()
            .map(|arg| arg.display().to_string())
            .collect(),
        success: status.success(),
        exit_status: status.to_string(),
        stdout,
        stderr,
    };

    fs_util::write(
        format_args!("{} summary", PrettyId(cmd, &id)),
        &summary_path,
        exec_result.to_string(),
    )?;

    let _ = fs_util::remove_file(format_args!("{} stdout", PrettyId(cmd, &id)), &stdout_path);
    let _ = fs_util::remove_file(format_args!("{} stderr", PrettyId(cmd, &id)), &stderr_path);

    Ok(exec_result)
}
