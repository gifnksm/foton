use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use crate::platform::windows::{
    direct_write::{DirectWriteError, DirectWriteFactory},
    property_store::{PropertyStore, PropertyStoreError},
};

use super::property_store::PropertyStoreKey;

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum FontInspectorError {
    #[display("failed to create DirectWrite factory for inspecting font")]
    CreateDirectWriteFactory {
        #[error(source)]
        source: DirectWriteError,
    },
    #[display("failed to check if the font file is supported: {path}", path = path.display())]
    CheckFontSupported {
        path: PathBuf,
        #[error(source)]
        source: DirectWriteError,
    },
    #[display("failed to get font title from property store: {path}", path = path.display())]
    GetFontTitleFromPropertyStore {
        path: PathBuf,
        #[error(source)]
        source: PropertyStoreError,
    },
}

#[derive(Debug)]
pub(crate) struct FontInspector {
    factory: DirectWriteFactory,
}

impl FontInspector {
    pub(crate) fn new() -> Result<Self, FontInspectorError> {
        let factory = DirectWriteFactory::new()
            .map_err(|source| FontInspectorError::CreateDirectWriteFactory { source })?;
        Ok(Self { factory })
    }

    pub(crate) fn is_supported_font_file(&self, path: &Path) -> Result<bool, FontInspectorError> {
        let analyze_result = (|| {
            let font_file = self.factory.font_file(path)?;
            font_file.analyze()
        })()
        .map_err(|source| {
            let path = path.to_owned();
            FontInspectorError::CheckFontSupported { path, source }
        })?;

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
        .map_err(|source| {
            let path = path.to_owned();
            FontInspectorError::GetFontTitleFromPropertyStore { path, source }
        })?;

        if title.is_empty() {
            return Ok(None);
        }

        Ok(Some(title))
    }
}
