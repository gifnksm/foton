use std::path::Path;

use color_eyre::eyre;

use crate::{fs_util, scenario::Scenario};

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

const CONFIG_TEMPLATE: &str = include_str!("../assets/sandbox.wsb.template");

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
    let host_target_dir = crate::host_target_dir()?;
    let host_base_dir = host_target_dir.join(format!(r"windows-sandbox\scenarios\{scenario}"));
    let host_bin_dir = host_base_dir.join(r"bin");
    let host_output_dir = host_base_dir.join(r"output");

    fs_util::create_dir_all("scenario", &host_base_dir)?;
    fs_util::create_dir_all("binary", &host_bin_dir)?;
    fs_util::create_dir_all("output", &host_output_dir)?;
    fs_util::copy(
        "xtask.exe",
        host_target_dir.join(r"debug\xtask.exe"),
        host_bin_dir.join("xtask.exe"),
    )?;
    fs_util::copy(
        "foton.exe",
        host_target_dir.join(r"debug\foton.exe"),
        host_bin_dir.join("foton.exe"),
    )?;

    let sandbox_base_dir = Path::new(r"C:\sandbox\");
    let sandbox_bin_dir = sandbox_base_dir.join(r"bin");
    let sandbox_xtask = sandbox_bin_dir.join(r"xtask.exe");
    let sandbox_foton = sandbox_bin_dir.join(r"foton.exe");
    let sandbox_output_dir = sandbox_base_dir.join(r"output");
    let login_command = format!(
        r"{} scenario run --scenario {} --foton-exe {} --output-dir {}",
        sandbox_xtask.display(),
        scenario,
        sandbox_foton.display(),
        sandbox_output_dir.display()
    );

    let config_path = host_base_dir.join("sandbox.wsb");
    let config_content = CONFIG_TEMPLATE
        .replace(
            "__HOST_BIN_DIR__",
            &escape_xml(&host_bin_dir.display().to_string()),
        )
        .replace(
            "__SANDBOX_BIN_DIR__",
            &escape_xml(&sandbox_bin_dir.display().to_string()),
        )
        .replace(
            "__HOST_OUTPUT_DIR__",
            &escape_xml(&host_output_dir.display().to_string()),
        )
        .replace(
            "__SANDBOX_OUTPUT_DIR__",
            &escape_xml(&sandbox_output_dir.display().to_string()),
        )
        .replace("__LOGON_COMMAND__", &escape_xml(&login_command));

    fs_util::write("sandbox config file", &config_path, config_content)?;

    eprintln!("Generated Windows Sandbox config:");
    eprintln!("  {}", config_path.display());
    eprintln!("Scenario:");
    eprintln!("  {scenario}");

    Ok(())
}
