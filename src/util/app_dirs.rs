use std::path::PathBuf;

use directories::ProjectDirs;

use crate::util::{
    fs::{self as fs_util, FsError},
    path::AbsolutePath,
};

#[derive(Debug)]
pub(crate) struct AppDirs {
    data_local_dir: AbsolutePath,
}

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
pub(crate) enum DirKind {
    #[display("data-local")]
    DataLocal,
}

impl AppDirs {
    pub(crate) fn from_directories() -> Result<Self, AppDirsError> {
        let dirs = ProjectDirs::from(APP_QUALIFIER, APP_ORGANIZATION, APP_APPLICATION)
            .ok_or(AppDirsError::GetProjectDirectories)?;
        let data_local_dir = dirs.data_local_dir();

        let Some(data_local_dir) = AbsolutePath::new(data_local_dir) else {
            return Err(AppDirsError::NotAbsolute {
                kind: DirKind::DataLocal,
                path: data_local_dir.to_owned(),
            });
        };

        fs_util::create_dir_all(&data_local_dir).map_err(|source| AppDirsError::CreateDir {
            kind: DirKind::DataLocal,
            path: data_local_dir.clone(),
            source,
        })?;

        Ok(Self { data_local_dir })
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(data_local_dir: AbsolutePath) -> Self {
        Self { data_local_dir }
    }

    pub(crate) fn data_local_dir(&self) -> &AbsolutePath {
        &self.data_local_dir
    }

    pub(crate) fn db_lock_file(&self) -> AbsolutePath {
        self.data_local_dir.join("db.lock")
    }
}
