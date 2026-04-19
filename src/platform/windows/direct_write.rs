use std::path::PathBuf;

use windows::Win32::Graphics::DirectWrite::{
    self, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_FACE_TYPE, DWRITE_FONT_FILE_TYPE, IDWriteFactory,
    IDWriteFontFile,
};
use windows_core::HSTRING;

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum DirectWriteError {
    #[display("failed to create DirectWrite factory")]
    CreateFactory {
        #[error(source)]
        source: windows_core::Error,
    },
    #[display("failed to create reference for font file: {path}", path = path.display())]
    CreateFontFileReference {
        path: PathBuf,
        #[error(source)]
        source: windows_core::Error,
    },
    #[display("failed to analyze font file: {path}", path = path.display())]
    AnalyzeFont {
        path: PathBuf,
        #[error(source)]
        source: windows_core::Error,
    },
}

#[derive(Debug)]
pub(crate) struct DirectWriteFactory {
    factory: IDWriteFactory,
}

impl DirectWriteFactory {
    pub(crate) fn new() -> Result<Self, DirectWriteError> {
        // SAFETY: This is an unsafe FFI call. We pass only the constant factory type argument.
        let factory: IDWriteFactory =
            unsafe { DirectWrite::DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }
                .map_err(|source| DirectWriteError::CreateFactory { source })?;
        Ok(Self { factory })
    }

    pub(crate) fn font_file<P>(&self, path: P) -> Result<DirectWriteFontFile, DirectWriteError>
    where
        P: Into<PathBuf>,
    {
        let path = path.into();
        // SAFETY: This is an unsafe FFI call. We pass a valid path pointer derived from a temporary
        // UTF-16 string that is kept alive for the duration of the call.
        let font_file = unsafe {
            self.factory
                .CreateFontFileReference(&HSTRING::from(path.as_path()), None)
        }
        .map_err(|source| {
            let path = path.clone();
            DirectWriteError::CreateFontFileReference { path, source }
        })?;
        Ok(DirectWriteFontFile { path, font_file })
    }
}

#[derive(Debug)]
pub(crate) struct DirectWriteFontFile {
    path: PathBuf,
    font_file: IDWriteFontFile,
}

#[derive(Debug, Default)]
pub(crate) struct DirectWriteFontFileAnalyzeResult {
    pub(crate) is_supported: bool,
}

impl DirectWriteFontFile {
    pub(crate) fn analyze(&self) -> Result<DirectWriteFontFileAnalyzeResult, DirectWriteError> {
        let mut is_supported = false.into();
        let mut font_file_type = DWRITE_FONT_FILE_TYPE::default();
        let mut font_face_type = DWRITE_FONT_FACE_TYPE::default();
        let mut number_of_faces = 0;
        // SAFETY: This is an unsafe FFI call. We pass valid pointers to local output variables that
        // remain alive for the duration of the call.
        unsafe {
            self.font_file.Analyze(
                &raw mut is_supported,
                &raw mut font_file_type,
                Some(&raw mut font_face_type),
                &raw mut number_of_faces,
            )
        }
        .map_err(|source| {
            let path = self.path.clone();
            DirectWriteError::AnalyzeFont { path, source }
        })?;
        Ok(DirectWriteFontFileAnalyzeResult {
            is_supported: is_supported.as_bool(),
        })
    }
}
