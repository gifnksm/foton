use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use snafu::{ResultExt as _, Snafu};

use crate::platform::windows::primitives::{
    direct_write::{
        DirectWriteFactory, DirectWriteFactoryError, DirectWriteFontSet,
        DirectWriteFontSetEntryError, DirectWriteFontSetError, DirectWriteLocalizedStringsError,
    },
    ui_languages::{UiLanguages, UiLanguagesError},
};

#[derive(Debug, Snafu)]
pub(crate) enum FontInspectorError {
    #[snafu(display("failed to create DirectWrite factory"))]
    CreateDirectWriteFactory { source: DirectWriteFactoryError },
    #[snafu(display("failed to get preferred UI languages"))]
    GetPreferredUiLanguages { source: UiLanguagesError },
    #[snafu(display("failed to create font set for font file: {path}", path = path.display()))]
    CreateFontSet {
        path: PathBuf,
        source: DirectWriteFontSetError,
    },
    #[snafu(display("failed to get font family name for font file: {path}", path = path.display()))]
    GetFontFamilyName {
        path: PathBuf,
        source: DirectWriteFontSetEntryError,
    },
    #[snafu(display("failed to get font face name for font file: {path}", path = path.display()))]
    GetFontFaceName {
        path: PathBuf,
        source: DirectWriteFontSetEntryError,
    },
    #[snafu(display("failed to pick localized font family name for font file: {path}", path = path.display()))]
    PickLocalizedFontFamilyName {
        path: PathBuf,
        source: DirectWriteLocalizedStringsError,
    },
    #[snafu(display("failed to pick localized font face name for font file: {path}", path = path.display()))]
    PickLocalizedFontFaceName {
        path: PathBuf,
        source: DirectWriteLocalizedStringsError,
    },
    #[snafu(display("no font face found in font file: {path}", path = path.display()))]
    NoFontFace { path: PathBuf },
}

#[derive(Debug)]
pub(crate) struct FontInspector {
    factory: DirectWriteFactory,
    preferred_languages: UiLanguages,
}

#[derive(Debug)]
pub(crate) struct InspectedFont {
    pub(crate) faces: Vec<InspectedFontFace>,
}

#[derive(Debug)]
pub(crate) struct InspectedFontFace {
    pub(crate) family: OsString,
    pub(crate) face: OsString,
}

impl FontInspector {
    pub(crate) fn new() -> Result<Self, FontInspectorError> {
        let factory = DirectWriteFactory::new().context(CreateDirectWriteFactorySnafu)?;
        let preferred_languages =
            UiLanguages::get_preferred().context(GetPreferredUiLanguagesSnafu)?;
        Ok(Self {
            factory,
            preferred_languages,
        })
    }

    pub(crate) fn inspect_font<P>(&self, path: P) -> Result<InspectedFont, FontInspectorError>
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref();
        let font_set = DirectWriteFontSet::from_file(&self.factory, path)
            .context(CreateFontSetSnafu { path })?;

        let faces = font_set
            .entries()
            .map(|entry| {
                let family = entry.family().context(GetFontFamilyNameSnafu { path })?;
                let family = family
                    .pick_localized_string(&self.preferred_languages)
                    .context(PickLocalizedFontFamilyNameSnafu { path })?;
                let face = entry.face().context(GetFontFaceNameSnafu { path })?;
                let face = face
                    .pick_localized_string(&self.preferred_languages)
                    .context(PickLocalizedFontFaceNameSnafu { path })?;
                Ok(InspectedFontFace { family, face })
            })
            .collect::<Result<Vec<_>, _>>()?;

        if faces.is_empty() {
            return Err(NoFontFaceSnafu { path }.build());
        }

        Ok(InspectedFont { faces })
    }
}
