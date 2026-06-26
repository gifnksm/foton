use std::path::{Path, PathBuf};

use snafu::{ResultExt as _, Snafu};

use crate::platform::windows::primitives::direct_write::{DirectWriteError, DirectWriteFactory};

#[derive(Debug, Snafu)]
pub(crate) enum FontInspectorError {
    #[snafu(display("failed to create DirectWrite factory for inspecting font"))]
    CreateDirectWriteFactory { source: DirectWriteError },
    #[snafu(display("failed to check if the font file is supported: {path}", path = path.display()))]
    CheckFontSupported {
        path: PathBuf,
        source: DirectWriteError,
    },
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
}
