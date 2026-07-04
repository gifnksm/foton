use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    range::{Range, RangeIter},
};

use snafu::{ResultExt as _, Snafu};

use crate::{
    package::{PackageDirs, PackageId},
    platform::windows::primitives::{
        direct_write::{
            DirectWriteFactory, DirectWriteFactoryError, DirectWriteFontSet,
            DirectWriteFontSetEntryError, DirectWriteFontSetError,
            DirectWriteLocalizedStringsError,
        },
        known_folders::{self, KnownFoldersError},
        ui_languages::{UiLanguages, UiLanguagesError},
    },
    util::app_dirs::AppDirs,
};

#[derive(Debug, Snafu)]
pub(crate) enum FontEnumeratorError {
    #[snafu(display("failed to create DirectWrite factory"))]
    CreateDirectWriteFactory { source: DirectWriteFactoryError },
    #[snafu(display("failed to get preferred UI languages"))]
    GetPreferredUiLanguages { source: UiLanguagesError },
    #[snafu(display("failed to get system fonts directory"))]
    GetSystemFontsDir { source: KnownFoldersError },
    #[snafu(display("failed to get system font set"))]
    GetSystemFontSet { source: DirectWriteFontSetError },
    #[snafu(display("failed to get font family name for system font set entry at index {index}"))]
    GetFontFamilyName {
        index: u32,
        source: DirectWriteFontSetEntryError,
    },
    #[snafu(display("failed to get font face name for system font set entry at index {index}"))]
    GetFontFaceName {
        index: u32,
        source: DirectWriteFontSetEntryError,
    },
    #[snafu(display(
        "failed to pick localized font family name for system font set entry at index {index}"
    ))]
    PickLocalizedFontFamilyName {
        index: u32,
        source: DirectWriteLocalizedStringsError,
    },
    #[snafu(display(
        "failed to pick localized font face name for system font set entry at index {index}"
    ))]
    PickLocalizedFontFaceName {
        index: u32,
        source: DirectWriteLocalizedStringsError,
    },
    #[snafu(display("failed to get font file path for system font set entry at index {index}"))]
    GetFontFilePath {
        index: u32,
        source: DirectWriteFontSetEntryError,
    },
}

#[derive(Debug)]
pub(crate) struct SystemFontEnumerator {
    factory: DirectWriteFactory,
    ui_languages: UiLanguages,
    system_fonts_dir: PathBuf,
}

impl SystemFontEnumerator {
    pub(crate) fn new() -> Result<Self, FontEnumeratorError> {
        let factory = DirectWriteFactory::new().context(CreateDirectWriteFactorySnafu)?;
        let ui_languages = UiLanguages::get_preferred().context(GetPreferredUiLanguagesSnafu)?;
        let system_fonts_dir = known_folders::system_fonts_dir().context(GetSystemFontsDirSnafu)?;
        let system_fonts_dir = canonicalize_path(system_fonts_dir);
        Ok(Self {
            factory,
            ui_languages,
            system_fonts_dir,
        })
    }

    pub(crate) fn fonts<'iter, 'dirs>(
        &'iter self,
        app_dirs: &'dirs AppDirs,
    ) -> Result<SystemFontIter<'iter, 'dirs>, FontEnumeratorError> {
        let font_set =
            DirectWriteFontSet::system_font_set(&self.factory).context(GetSystemFontSetSnafu)?;
        let count = font_set.entry_count();

        Ok(SystemFontIter {
            ui_languages: &self.ui_languages,
            system_fonts_dir: &self.system_fonts_dir,
            app_dirs,
            font_set,
            index: Range::from(0..count).into_iter(),
        })
    }
}

#[derive(Debug)]
pub(crate) struct SystemFontIter<'iter, 'dirs> {
    ui_languages: &'iter UiLanguages,
    system_fonts_dir: &'iter Path,
    app_dirs: &'dirs AppDirs,
    font_set: DirectWriteFontSet,
    index: RangeIter<u32>,
}

impl Iterator for SystemFontIter<'_, '_> {
    type Item = Result<SystemFont, FontEnumeratorError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.index.next().map(|index| {
            let entry = self.font_set.entry_at(index);
            let family = entry
                .family()
                .context(GetFontFamilyNameSnafu { index })?
                .pick_localized_string(self.ui_languages)
                .context(PickLocalizedFontFamilyNameSnafu { index })?;

            let face = entry
                .face()
                .context(GetFontFaceNameSnafu { index })?
                .pick_localized_string(self.ui_languages)
                .context(PickLocalizedFontFaceNameSnafu { index })?;

            let path = entry.path().context(GetFontFilePathSnafu { index })?;
            let path = path.map(canonicalize_path);

            let location = match path {
                // Windows system fonts are installed under the system font directory, so the file
                // path alone is enough to distinguish them from user fonts here.
                Some(path) if path.starts_with(self.system_fonts_dir) => {
                    FontLocation::SystemDir { path }
                }
                Some(path) => match PackageDirs::from_font_path(self.app_dirs, &path) {
                    Some(pkg_dirs) => FontLocation::PackageDir {
                        pkg_id: pkg_dirs.pkg_id().clone(),
                        path,
                    },
                    // Windows system fonts live under the system font directory. Any other local
                    // font that is visible here and not owned by foton is treated as a user font.
                    None => FontLocation::UserDir { path },
                },
                // Some DirectWrite entries are not backed by a local file path, so they cannot be
                // attributed to system, user, or foton-managed storage.
                None => FontLocation::Unknown,
            };

            Ok(SystemFont {
                family,
                face,
                location,
            })
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SystemFont {
    pub(crate) family: OsString,
    pub(crate) face: OsString,
    pub(crate) location: FontLocation,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum FontLocation {
    PackageDir { path: PathBuf, pkg_id: PackageId },
    SystemDir { path: PathBuf },
    UserDir { path: PathBuf },
    Unknown,
}

fn canonicalize_path<P>(path: P) -> PathBuf
where
    P: AsRef<Path>,
{
    let path = path.as_ref();
    dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}
