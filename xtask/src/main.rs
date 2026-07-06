#[cfg(not(windows))]
compile_error!("foton is supported on Windows only.");

use clap::Parser as _;
use color_eyre::eyre;

use crate::command::Args;

mod command;
mod report;
mod sandbox;
mod scenario;
mod util;

fn main() -> eyre::Result<()> {
    let args = Args::parse();

    color_eyre::install()?;
    report::init_panic_hook();

    command::dispatch(&args)?;

    Ok(())
}
