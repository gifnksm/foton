use cargo_metadata::camino::Utf8PathBuf;
use color_eyre::eyre::{self, WrapErr as _, eyre};
use tempfile::TempDir;

use crate::{
    bootstrap::SandboxBootstrapConfig,
    report::RunId,
    scenario::RunArgs,
    util::{
        build,
        env::{self as env_util},
        fs as fs_util,
    },
};

#[derive(Debug)]
pub(in crate::scenario) struct ScenarioParameters {
    pub(in crate::scenario) foton_exe: Utf8PathBuf,
    pub(in crate::scenario) fixture_dir: Utf8PathBuf,
    pub(in crate::scenario) output_dir: Utf8PathBuf,
    pub(in crate::scenario) run_id: RunId,
}

impl ScenarioParameters {
    pub(in crate::scenario) fn from_args(args: &RunArgs) -> eyre::Result<(Option<TempDir>, Self)> {
        let RunArgs {
            foton_exe,
            fixture_dir,
            output_dir,
        } = args;

        let foton_exe = if let Some(path) = foton_exe {
            path.clone()
        } else {
            build::build_foton_exe()?
        };
        let (tempdir_guard, output_dir) = if let Some(path) = output_dir {
            fs_util::create_dir_all("output directory", path)?;
            (None, path.clone())
        } else {
            let tempdir = TempDir::new().wrap_err("failed to create temporary output directory")?;
            let path = Utf8PathBuf::from_path_buf(tempdir.path().to_owned()).map_err(|path| {
                eyre!(
                    "failed to convert temporary output directory path to UTF-8: {}",
                    path.display()
                )
            })?;
            (Some(tempdir), path)
        };
        let fixture_dir = if let Some(path) = fixture_dir {
            path.clone()
        } else {
            env_util::fixture_dir()?
        };
        Ok((
            tempdir_guard,
            Self {
                foton_exe,
                fixture_dir,
                output_dir,
                run_id: RunId::new(),
            },
        ))
    }
}

impl From<&SandboxBootstrapConfig> for ScenarioParameters {
    fn from(config: &SandboxBootstrapConfig) -> Self {
        Self {
            foton_exe: config.foton_exe.clone(),
            fixture_dir: config.fixture_dir.clone(),
            output_dir: config.output_dir.clone(),
            run_id: config.run_id,
        }
    }
}
