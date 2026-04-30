use std::path::{Path, PathBuf};

use directories::ProjectDirs;

use crate::{
    registry::RegistryId,
    util::{
        fs::{self as fs_util, FsError},
        path::AbsolutePath,
    },
};

const APP_QUALIFIER: &str = "";
const APP_ORGANIZATION: &str = "io.github.gifnksm";
const APP_APPLICATION: &str = "foton";

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum AppDirsError {
    #[display("failed to get project directories")]
    GetProjectDirectories,
    #[display("{kind} directory is not absolute: {path}", path = path.display())]
    NotAbsolute { kind: DirKind, path: PathBuf },
    #[display("failed to create {kind} directory: {path}", path = path.display())]
    CreateDir {
        kind: DirKind,
        path: AbsolutePath,
        #[error(source)]
        source: FsError,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, derive_more::IsVariant, derive_more::Display)]
#[display(rename_all = "kebab-case")]
pub(crate) enum DirKind {
    DataLocal,
    Config,
    Cache,
}

#[derive(Debug)]
#[expect(clippy::struct_field_names)]
pub(crate) struct AppDirs {
    data_local_dir: AbsolutePath,
    config_dir: AbsolutePath,
    cache_dir: AbsolutePath,
}

fn to_absolute(path: &Path, kind: DirKind) -> Result<AbsolutePath, AppDirsError> {
    AbsolutePath::new(path).ok_or(AppDirsError::NotAbsolute {
        kind,
        path: path.to_owned(),
    })
}

fn create_dir(path: &AbsolutePath, kind: DirKind) -> Result<(), AppDirsError> {
    fs_util::create_dir_all(path).map_err(|source| AppDirsError::CreateDir {
        kind,
        path: path.clone(),
        source,
    })
}

impl AppDirs {
    pub(crate) fn from_directories() -> Result<Self, AppDirsError> {
        let dirs = ProjectDirs::from(APP_QUALIFIER, APP_ORGANIZATION, APP_APPLICATION)
            .ok_or(AppDirsError::GetProjectDirectories)?;
        let data_local_dir = dirs.data_local_dir();
        let cache_dir = dirs.cache_dir();

        let data_local_dir = to_absolute(data_local_dir, DirKind::DataLocal)?;
        let config_dir = to_absolute(dirs.config_dir(), DirKind::Config)?;
        let cache_dir = to_absolute(cache_dir, DirKind::Cache)?;
        create_dir(&data_local_dir, DirKind::DataLocal)?;
        create_dir(&config_dir, DirKind::Config)?;
        create_dir(&cache_dir, DirKind::Cache)?;

        Ok(Self {
            data_local_dir,
            config_dir,
            cache_dir,
        })
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(base_dir: &Path) -> Result<Self, AppDirsError> {
        let base_dir = to_absolute(base_dir, DirKind::DataLocal)?;
        let data_local_dir = base_dir.join("data");
        let config_dir = base_dir.join("config");
        let cache_dir = base_dir.join("cache");
        create_dir(&data_local_dir, DirKind::DataLocal)?;
        create_dir(&config_dir, DirKind::Config)?;
        create_dir(&cache_dir, DirKind::Cache)?;
        Ok(Self {
            data_local_dir,
            config_dir,
            cache_dir,
        })
    }

    pub(crate) fn data_local_dir(&self) -> &AbsolutePath {
        &self.data_local_dir
    }

    pub(crate) fn db_json_file(&self) -> AbsolutePath {
        self.data_local_dir.join("db.json")
    }

    pub(crate) fn db_lock_file(&self) -> AbsolutePath {
        self.data_local_dir.join("db.lock")
    }

    pub(crate) fn config_file(&self) -> AbsolutePath {
        self.config_dir.join("config.toml")
    }

    pub(crate) fn registry_cache_dir(&self, id: &RegistryId) -> AbsolutePath {
        self.cache_dir.join("registries").join(id.as_str())
    }
}
