use std::{fmt::Write as _, fs};

use cargo_metadata::camino::Utf8PathBuf;
use color_eyre::eyre::{self, WrapErr as _};

mod help_check;

#[derive(Debug, Clone, PartialEq, Eq, clap::ValueEnum, derive_more::Display)]
#[display(rename_all = "kebab-case")]
pub(crate) enum Scenario {
    HelpCheck,
}

#[derive(clap::Subcommand)]
pub(crate) enum ScenarioCommand {
    Run {
        #[clap(long)]
        scenario: Scenario,
        #[clap(flatten)]
        args: RunArgs,
    },
}

#[derive(clap::Args)]
pub(crate) struct RunArgs {
    #[clap(long)]
    foton_exe: Utf8PathBuf,
    #[clap(long)]
    output_dir: Utf8PathBuf,
}

pub(crate) fn dispatch(command: &ScenarioCommand) -> eyre::Result<()> {
    match command {
        ScenarioCommand::Run { scenario, args } => run(scenario, args),
    }
}

pub(crate) fn run(scenario: &Scenario, args: &RunArgs) -> eyre::Result<()> {
    fs::create_dir_all(&args.output_dir)
        .wrap_err_with(|| format!("failed to create output directory: {}", args.output_dir))?;

    let result_path = args.output_dir.join("result.txt");
    let res = match scenario {
        Scenario::HelpCheck => help_check::run(args),
    };

    let result = match &res {
        Ok(()) => "PASS".to_owned(),
        Err(err) => {
            let mut output = "FAIL\n".to_owned();
            writeln!(&mut output, "{}", err)?;
            let mut source = err.source();
            while let Some(err) = source {
                writeln!(&mut output, "  caused by: {err}")?;
                source = err.source();
            }
            output
        }
    };

    fs::write(&result_path, result)
        .wrap_err_with(|| format!("failed to write to file: {}", result_path))?;

    res
}
