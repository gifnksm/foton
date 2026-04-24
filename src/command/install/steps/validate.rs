use std::collections::HashSet;

use crate::{
    package::FontEntry,
    platform::windows::services::font::{FontValidator, FontValidatorError},
    util::{
        fs as fs_util,
        path::{AbsolutePath, FileName},
        reporter::{ReportErrorExt as _, Reporter},
    },
};

#[derive(Debug, derive_more::Display, derive_more::Error, derive_more::From)]
pub(crate) enum ValidationWarning {
    #[display("removing unsupported font file: {path}", path = path.display())]
    RemovingUnsupportedFontFile { path: AbsolutePath },
    #[display("failed to remove unsupported font file: {path}; manual cleanup may be required", path = path.display())]
    RemoveUnsupportedFontFile {
        path: AbsolutePath,
        #[error(source)]
        source: fs_util::FsError,
    },
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum ValidationError {
    #[display("failed to create font validator")]
    CreateValidator {
        #[error(source)]
        source: FontValidatorError,
    },
    #[display("failed to validate font file: {file_name}", file_name = file_name.display())]
    ValidateFont {
        file_name: FileName,
        #[error(source)]
        source: FontValidatorError,
    },
    #[display("duplicate font name found in package: {title}")]
    DuplicateFontName { title: String },
}

pub(in crate::command::install) fn validate_and_prune_fonts(
    reporter: &Reporter,
    fonts_dir: &AbsolutePath,
    file_names: &[FileName],
) -> Result<Vec<FontEntry>, Box<ValidationError>> {
    let mut valid_entries = vec![];
    let mut valid_entry_titles = HashSet::new();
    let validator =
        FontValidator::new().map_err(|source| ValidationError::CreateValidator { source })?;

    for file_name in file_names {
        let Some(entry) = validator
            .validate_font(fonts_dir, file_name)
            .map_err(|source| {
                let file_name = file_name.clone();
                ValidationError::ValidateFont { file_name, source }
            })?
        else {
            let path = fonts_dir.join(file_name);
            reporter.report_warn(&ValidationWarning::RemovingUnsupportedFontFile {
                path: path.clone(),
            } as &dyn std::error::Error);
            let _ = fs_util::remove_file(&path)
                .map_err(|source| ValidationWarning::RemoveUnsupportedFontFile { path, source })
                .report_err_as_warn(reporter);
            continue;
        };

        if !valid_entry_titles.insert(entry.title().to_lowercase()) {
            let title = entry.title().to_owned();
            return Err(ValidationError::DuplicateFontName { title }.into());
        }

        valid_entries.push(entry);
    }

    Ok(valid_entries)
}
