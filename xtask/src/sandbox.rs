use std::path::Path;

use color_eyre::eyre;
use wsbx::{SandboxConfig, config::MappedFolder};

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
    let logon_command = format!(
        r"{} scenario run --scenario {} --foton-exe {} --output-dir {}",
        sandbox_xtask.display(),
        scenario,
        sandbox_foton.display(),
        sandbox_output_dir.display()
    );

    let config_path = host_base_dir.join("sandbox.wsb");
    let config = SandboxConfig::new()
        .mapped_folder(
            MappedFolder::new(host_bin_dir)
                .sandbox_folder(sandbox_bin_dir)
                .read_only(true),
        )
        .mapped_folder(
            MappedFolder::new(host_output_dir)
                .sandbox_folder(sandbox_output_dir)
                .read_only(false),
        )
        .logon_command(logon_command)
        .to_pretty_os_string();

    fs_util::write(
        "sandbox config file",
        &config_path,
        config.as_encoded_bytes(),
    )?;

    eprintln!("Generated Windows Sandbox config:");
    eprintln!("  {}", config_path.display());
    eprintln!("Scenario:");
    eprintln!("  {scenario}");

    Ok(())
}
