use std::{
    io,
    path::{Path, PathBuf},
};

use clap::CommandFactory as _;
use snafu::{ResultExt as _, Snafu};

use crate::cli::args::Args;

#[derive(Debug, Snafu)]
pub(crate) enum GenerateManError {
    #[snafu(display("failed to generate man page: {output_dir}", output_dir = output_dir.display()))]
    GenerateMan {
        output_dir: PathBuf,
        source: io::Error,
    },
}

pub(crate) fn generate_man<P>(output_dir: P) -> Result<(), GenerateManError>
where
    P: AsRef<Path>,
{
    let output_dir = output_dir.as_ref();
    clap_mangen::generate_to(Args::command(), output_dir).context(GenerateManSnafu { output_dir })
}
