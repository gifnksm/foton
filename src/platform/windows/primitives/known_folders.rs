use std::{ffi::OsString, os::windows::ffi::OsStringExt as _, path::PathBuf};

use snafu::{ResultExt as _, Snafu};
use windows::Win32::{
    System::Com,
    UI::Shell::{self, FOLDERID_Fonts, KF_FLAG_DEFAULT},
};

#[derive(Debug, Snafu)]
pub(crate) enum KnownFoldersError {
    #[snafu(display("failed to get system fonts directory"))]
    GetSystemFontsDir { source: windows_core::Error },
}

pub(crate) fn system_fonts_dir() -> Result<PathBuf, KnownFoldersError> {
    get_dir(&FOLDERID_Fonts)
}

fn get_dir(folder_id: &windows_core::GUID) -> Result<PathBuf, KnownFoldersError> {
    let path;
    // SAFETY: `SHGetKnownFolderPath` returns a COM-task-allocated, NUL-terminated UTF-16 path
    // buffer that remains valid until it is released with `CoTaskMemFree`. We read the string and
    // copy it into an owned `PathBuf` before freeing it, and we do not access `path_ptr` after
    // `CoTaskMemFree` invalidates the allocation.
    unsafe {
        let path_ptr = Shell::SHGetKnownFolderPath(folder_id, KF_FLAG_DEFAULT, None)
            .context(GetSystemFontsDirSnafu)?;
        path = PathBuf::from(OsString::from_wide(path_ptr.as_wide()));
        Com::CoTaskMemFree(Some(path_ptr.as_ptr().cast()));
    }
    Ok(path)
}
