use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use snafu::{ResultExt as _, Snafu};

use crate::platform::windows::primitives::{
    direct_write::{DirectWriteError, DirectWriteFactory},
    property_store::{PropertyStore, PropertyStoreError, PropertyStoreKey},
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
}

#[derive(Debug)]
pub(crate) struct FontInspector {
    factory: DirectWriteFactory,
}

impl FontInspector {
    pub(crate) fn new() -> Result<Self, FontInspectorError> {
        let factory = DirectWriteFactory::new().context(CreateDirectWriteFactorySnafu)?;
        Ok(Self { factory })
    }

    pub(crate) fn is_supported_font_file(&self, path: &Path) -> Result<bool, FontInspectorError> {
        let analyze_result = (|| {
            let font_file = self.factory.font_file(path)?;
            font_file.analyze()
        })()
        .context(CheckFontSupportedSnafu { path })?;

        Ok(analyze_result.is_supported)
    }

    #[expect(clippy::unused_self)]
    pub(crate) fn get_font_title(
        &self,
        path: &Path,
    ) -> Result<Option<OsString>, FontInspectorError> {
        let title = (|| {
            let store = PropertyStore::new(path)?;
            store.get_property_as_os_string(PropertyStoreKey::Title)
        })()
        .context(GetFontTitleFromPropertyStoreSnafu { path })?;

        if title.is_empty() {
            return Ok(None);
        }

        Ok(Some(title))
    }
}
