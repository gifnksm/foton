use std::path::{Path, PathBuf};

use snafu::{OptionExt as _, ResultExt as _, Snafu};

use crate::{
    platform::windows::primitives::{
        direct_write::{DirectWriteError, DirectWriteFactory},
        property_store::{PropertyStore, PropertyStoreError, PropertyStoreKey},
    },
    util::{macros::concat_line, path::FileName},
};

#[derive(Debug, Snafu)]
pub(crate) enum FontInspectorError {
    #[snafu(display("failed to create DirectWrite factory for inspecting font"))]
    CreateDirectWriteFactory { source: DirectWriteError },
    #[snafu(display("failed to check if the font file is supported: {path}", path = path.display()))]
    CheckFontSupported {
        path: PathBuf,
        source: DirectWriteError,
    },
    #[snafu(display("failed to get font title from property store: {path}", path = path.display()))]
    GetFontTitleFromPropertyStore {
        path: PathBuf,
        source: PropertyStoreError,
    },
    #[snafu(display(
        concat_line!(
            "failed to convert font title to string for file: {path}",
            "the title may contain invalid UTF-8",
        ),
        path = path.display(),
    ))]
    ConvertFontTitleToString { path: PathBuf },
}

#[derive(Debug)]
pub(crate) struct FontInspector {
    factory: DirectWriteFactory,
}

#[derive(Debug)]
pub(crate) struct InspectedFont {}

impl FontInspector {
    pub(crate) fn new() -> Result<Self, FontInspectorError> {
        let factory = DirectWriteFactory::new().context(CreateDirectWriteFactorySnafu)?;
        Ok(Self { factory })
    }

    pub(crate) fn inspect_font<P>(
        &self,
        path: P,
    ) -> Result<Option<InspectedFont>, FontInspectorError>
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref();
        if !self.is_supported_font_file(path)? {
            return Ok(None);
        }
        Ok(Some(InspectedFont {}))
    }

    fn is_supported_font_file(&self, path: &Path) -> Result<bool, FontInspectorError> {
        let analyze_result = (|| {
            let font_file = self.factory.font_file(path)?;
            font_file.analyze()
        })()
        .context(CheckFontSupportedSnafu { path })?;

        Ok(analyze_result.is_supported)
    }

    // Title lookup is intentionally kept separate from `inspect_font`.
    //
    // To read the title, we first obtain a property store with
    // `SHGetPropertyStoreFromParsingName`. That API breaks on
    // extended-length paths such as `\\?\C:\...`, so call this only when
    // the caller actually needs the font title.
    #[expect(clippy::unused_self)]
    pub(crate) fn get_font_title<P>(
        &self,
        path: P,
        file_name: &FileName,
    ) -> Result<String, FontInspectorError>
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref();
        let mut title = (|| {
            let store = PropertyStore::new(path)?;
            store.get_property_as_os_string(PropertyStoreKey::Title)
        })()
        .context(GetFontTitleFromPropertyStoreSnafu { path })?;

        if title.is_empty() {
            title = file_name.to_os_string();
        }

        let title = title
            .into_string()
            .ok()
            .context(ConvertFontTitleToStringSnafu { path })?;

        Ok(title)
    }
}
