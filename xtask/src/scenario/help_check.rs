use color_eyre::eyre;

use crate::scenario::ScenarioContext;

pub(super) fn run(cx: &ScenarioContext<'_>) -> eyre::Result<()> {
    cx.exec_foton(|cmd| {
        cmd.arg("--help");
    })?
    .ensure_success()?
    .ensure_stdout(|stdout| stdout.contains("Usage:"))?
    .ensure_stderr(str::is_empty)?;

    Ok(())
}
