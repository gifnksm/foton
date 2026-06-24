use std::path::{Path, PathBuf};

use snafu::{OptionExt as _, ResultExt as _, Snafu};

use crate::{
    engine::ExtractedFile,
    package::FontEntry,
    platform::windows::primitives::font_inspector::{FontInspector, FontInspectorError},
    util::macros::concat_line,
};

#[derive(Debug, Snafu)]
pub(crate) enum FontValidatorError {
    #[snafu(display("failed to create FontInspector"))]
    CreateFontInspector {
        #[snafu(source(from(FontInspectorError, Box::new)))]
        source: Box<FontInspectorError>,
    },
    #[snafu(display("failed to check if font file is supported: {path}", path = path.display()))]
    CheckSupport {
        path: PathBuf,
        #[snafu(source(from(FontInspectorError, Box::new)))]
        source: Box<FontInspectorError>,
    },
    #[snafu(display("failed to get font title for file: {path}", path = path.display()))]
    GetFontTitle {
        path: PathBuf,
        #[snafu(source(from(FontInspectorError, Box::new)))]
        source: Box<FontInspectorError>,
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
pub(crate) struct FontValidator {
    inspector: FontInspector,
}

impl FontValidator {
    pub(crate) fn new() -> Result<Self, FontValidatorError> {
        let inspector = FontInspector::new().context(CreateFontInspectorSnafu)?;
        Ok(Self { inspector })
    }

    pub(crate) fn validate_font<P>(
        &self,
        fonts_dir: P,
        file: &ExtractedFile,
    ) -> Result<Option<FontEntry>, FontValidatorError>
    where
        P: AsRef<Path>,
    {
        let fonts_dir = fonts_dir.as_ref();
        let path = fonts_dir.join(file.file_name());
        let supported = self
            .inspector
            .is_supported_font_file(&path)
            .context(CheckSupportSnafu { path: &path })?;

        if !supported {
            return Ok(None);
        }

        let title = file
            .title()
            .map(ToOwned::to_owned)
            .map_or_else(|| self.get_font_title(&path, file), Ok)?;

        Ok(Some(FontEntry::new(title, file.file_name())))
    }

    fn get_font_title(
        &self,
        path: &Path,
        file: &ExtractedFile,
    ) -> Result<String, FontValidatorError> {
        let title = self
            .inspector
            .get_font_title(path)
            .context(GetFontTitleSnafu { path })?
            .unwrap_or_else(|| file.file_name().to_os_string());

        let title = title
            .into_string()
            .ok()
            .context(ConvertFontTitleToStringSnafu { path })?;

        Ok(title)
    }
}
