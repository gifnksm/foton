use std::{fs, path::Path};

use color_eyre::eyre::{self, WrapErr as _};

use crate::{SANDBOX_TARGET_DIR, scenario::Scenario};

#[derive(clap::Subcommand)]
pub(crate) enum SandboxCommand {
    Generate {
        #[clap(long)]
        scenario: Scenario,
    },
}

pub(crate) fn dispatch(command: &SandboxCommand) -> eyre::Result<()> {
    match command {
        SandboxCommand::Generate { scenario } => generate(scenario),
    }
}

const TEMPLATE: &str = include_str!("../assets/sandbox.wsb.template");

fn escape_xml(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn generate(scenario: &Scenario) -> eyre::Result<()> {
    let relative_output_dir = format!(r"windows-sandbox\scenarios\{scenario}");
    let relative_xtask = r"debug\xtask.exe";
    let relative_foton = r"debug\foton.exe";

    let host_target = crate::host_target_dir()?;
    let host_foton = host_target.join(relative_foton);
    let host_output_dir = host_target.join(&relative_output_dir);

    let sandbox_target = Path::new(SANDBOX_TARGET_DIR);
    let sandbox_xtask = sandbox_target.join(relative_xtask);
    let sandbox_foton = sandbox_target.join(relative_foton);
    let sandbox_output_dir = sandbox_target.join(&relative_output_dir);
    let login_command = format!(
        r"{} scenario run --scenario {} --foton-exe {} --output-dir {}",
        sandbox_xtask.display(),
        scenario,
        sandbox_foton.display(),
        sandbox_output_dir.display()
    );

    let wsb = TEMPLATE
        .replace(
            "__HOST_TARGET__",
            &escape_xml(&host_target.display().to_string()),
        )
        .replace("__LOGON_COMMAND__", &escape_xml(&login_command));

    fs::create_dir_all(&host_output_dir).wrap_err_with(|| {
        format!(
            "failed to create output directory: {}",
            host_output_dir.display()
        )
    })?;
    let output_path = host_output_dir.join("sandbox.wsb");
    fs::write(&output_path, wsb.as_bytes())
        .wrap_err_with(|| format!("failed to write output file: {}", output_path.display()))?;

    eprintln!("Generated Windows Sandbox config:");
    eprintln!("  {}", output_path.display());
    eprintln!("Scenario:");
    eprintln!("  {scenario}");

    if !host_foton.is_file() {
        eprintln!();
        eprintln!(
            "WARNING: foton.exe does not exist: {}",
            host_foton.display()
        );
    }

    Ok(())
}
