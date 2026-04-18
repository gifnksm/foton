use std::path::{Path, PathBuf};

use color_eyre::eyre::{self, OptionExt as _};
use directories::ProjectDirs;

#[derive(Debug)]
pub(crate) struct AppDirs {
    data_dir: PathBuf,
}

const APP_QUALIFIER: &str = "";
const APP_ORGANIZATION: &str = "io.github.gifnksm";
const APP_APPLICATION: &str = "foton";

impl AppDirs {
    pub(crate) fn from_directories() -> eyre::Result<Self> {
        let dirs = ProjectDirs::from(APP_QUALIFIER, APP_ORGANIZATION, APP_APPLICATION)
            .ok_or_eyre("failed to get project directories")?;
        Ok(Self {
            data_dir: dirs.data_dir().to_path_buf(),
        })
    }

    pub(crate) fn data_dir(&self) -> &Path {
        &self.data_dir
    }
}
