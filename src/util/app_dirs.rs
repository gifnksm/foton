use std::path::PathBuf;

use directories::ProjectDirs;

use crate::util::path::AbsolutePath;

#[derive(Debug)]
pub(crate) struct AppDirs {
    data_dir: AbsolutePath,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, derive_more::IsVariant, derive_more::Display)]
pub(crate) enum DirKind {
    #[display("data")]
    Data,
}

impl AppDirs {
    pub(crate) fn from_directories() -> Result<Self, AppDirsError> {
        let dirs = ProjectDirs::from(APP_QUALIFIER, APP_ORGANIZATION, APP_APPLICATION)
            .ok_or(AppDirsError::GetProjectDirectories)?;
        let data_dir = dirs.data_dir();
        let Some(data_dir) = AbsolutePath::new(dirs.data_dir()) else {
            return Err(AppDirsError::NotAbsolute {
                kind: DirKind::Data,
                path: data_dir.to_owned(),
            });
        };
        Ok(Self { data_dir })
    }

    pub(crate) fn data_dir(&self) -> &AbsolutePath {
        &self.data_dir
    }
}
