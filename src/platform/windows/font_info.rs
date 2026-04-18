use std::path::Path;

use color_eyre::eyre::{self, WrapErr as _};
use windows::Win32::{
    Graphics::DirectWrite::{
        self, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_FACE_TYPE, DWRITE_FONT_FILE_TYPE,
        IDWriteFactory,
    },
    Storage::EnhancedStorage,
    UI::Shell::PropertiesSystem::{self, GPS_DEFAULT, IPropertyStore},
};
use windows_core::{BSTR, HSTRING};

#[derive(Debug, Clone)]
pub(crate) struct FontInspector {
    factory: IDWriteFactory,
}

impl FontInspector {
    pub(crate) fn new() -> eyre::Result<Self> {
        let factory: IDWriteFactory =
            // SAFETY: This is an unsafe FFI call. We pass only the constant factory type argument.
            unsafe { DirectWrite::DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }
                    .wrap_err("failed to create DirectWrite factory")?;
        Ok(Self { factory })
    }

    pub(crate) fn is_supported_font_file(&self, path: &Path) -> eyre::Result<bool> {
        // SAFETY: This is an unsafe FFI call. We pass a valid path pointer derived from a temporary
        // UTF-16 string that is kept alive for the duration of the call.
        let font_file = unsafe {
            self.factory
                .CreateFontFileReference(&HSTRING::from(path), None)
        }
        .wrap_err_with(|| format!("failed to create font file reference: {}", path.display()))?;
        let mut is_supported = false.into();
        let mut font_file_type = DWRITE_FONT_FILE_TYPE::default();
        let mut font_face_type = DWRITE_FONT_FACE_TYPE::default();
        let mut number_of_faces = 0;
        // SAFETY: This is an unsafe FFI call. We pass valid pointers to local output variables that
        // remain alive for the duration of the call.
        unsafe {
            font_file.Analyze(
                &raw mut is_supported,
                &raw mut font_file_type,
                Some(&raw mut font_face_type),
                &raw mut number_of_faces,
            )
        }
        .wrap_err_with(|| format!("failed to analyze font file: {}", path.display()))?;

        Ok(is_supported.as_bool())
    }
}

pub(crate) fn get_font_title(path: &Path) -> eyre::Result<Option<String>> {
    let prop_store: IPropertyStore =
        // SAFETY: This is an unsafe FFI call. We pass a valid path pointer derived from a
        // temporary UTF-16 string that is kept alive for the duration of the call.
        unsafe {
            PropertiesSystem::SHGetPropertyStoreFromParsingName(
                &HSTRING::from(path),
                None,
                GPS_DEFAULT,
            )
        }
    .wrap_err_with(|| format!("failed to get property store for font: {}", path.display()))?;
    // SAFETY: This is an unsafe FFI call. We pass a valid pointer to the constant property key.
    let value = unsafe { prop_store.GetValue(&EnhancedStorage::PKEY_Title) }.map_err(|e| {
        eyre::eyre!(
            "failed to get title property for font: {}: {e}",
            path.display()
        )
    })?;
    if value.is_empty() {
        return Ok(None);
    }

    let value = BSTR::try_from(&value).wrap_err_with(|| {
        format!(
            "title property is not a string for font: {}",
            path.display()
        )
    })?;
    if value.is_empty() {
        return Ok(None);
    }
    Ok(Some(value.to_string()))
}
