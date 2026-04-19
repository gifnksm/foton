use std::path::{Path, PathBuf};

use crate::{
    package::FontEntry,
    platform::windows::primitives::font_inspector::{FontInspector, FontInspectorError},
    util::path::FileName,
};

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum FontValidatorError {
    #[display("failed to create FontInspector")]
    CreateFontInspector {
        #[error(source)]
        source: Box<FontInspectorError>,
    },
    #[display("failed to check if font file is supported: {path}", path = path.display())]
    CheckSupport {
        path: PathBuf,
        #[error(source)]
        source: Box<FontInspectorError>,
    },
    #[display("failed to get font title for file: {path}", path = path.display())]
    GetFontTitle {
        path: PathBuf,
        #[error(source)]
        source: Box<FontInspectorError>,
    },
    #[display(
        "failed to convert font title to string for file: {path}; the title may contain invalid UTF-8", path = path.display()
    )]
    ConvertFontTitleToString { path: PathBuf },
}

#[derive(Debug)]
pub(crate) struct FontValidator {
    inspector: FontInspector,
}

impl FontValidator {
    pub(crate) fn new() -> Result<Self, FontValidatorError> {
        let inspector = FontInspector::new().map_err(|source| {
            let source = Box::new(source);
            FontValidatorError::CreateFontInspector { source }
        })?;
        Ok(Self { inspector })
    }

    pub(crate) fn validate_font<P>(
        &self,
        fonts_dir: P,
        file_name: &FileName,
    ) -> Result<Option<FontEntry>, FontValidatorError>
    where
        P: AsRef<Path>,
    {
        let fonts_dir = fonts_dir.as_ref();
        let path = fonts_dir.join(file_name);
        let supported = self
            .inspector
            .is_supported_font_file(&path)
            .map_err(|source| {
                let path = path.clone();
                let source = Box::new(source);
                FontValidatorError::CheckSupport { path, source }
            })?;

        if !supported {
            return Ok(None);
        }

        let title = self
            .inspector
            .get_font_title(&path)
            .map_err(|source| {
                let path = path.clone();
                let source = Box::new(source);
                FontValidatorError::GetFontTitle { path, source }
            })?
            .unwrap_or_else(|| file_name.to_os_string());

        let title = title.to_str().ok_or_else(|| {
            let path = path.clone();
            FontValidatorError::ConvertFontTitleToString { path }
        })?;

        Ok(Some(FontEntry::new(title, file_name)))
    }
}
