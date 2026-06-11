use std::{collections::HashSet, marker::PhantomData, path::PathBuf};

use snafu::{ResultExt as _, Snafu};

use crate::{
    cli::{
        context::ReportContext,
        reporter::{
            ErrorReportExt as _, NeverReport, ReportScope, ScopeResultErrorExt as _,
            ScopeResultWarnExt as _, SubReportScope,
        },
    },
    package::FontEntry,
    platform::windows::services::font::{FontValidator, FontValidatorError},
    util::{
        fs as fs_util,
        macros::concat_line,
        path::{AbsolutePath, FileName},
    },
};

#[derive(Debug, Default)]
struct ValidationScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for ValidationScope<S>
where
    S: ReportScope,
{
    type NoticeReportValue = NeverReport;
    type WarnReportValue = ValidationWarnReport;
    type ErrorReportValue = ValidationErrorReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for ValidationScope<S> where S: ReportScope {}

#[derive(Debug, Snafu)]
enum ValidationWarnReport {
    #[snafu(display("unsupported font file found: {path}", path = path.display()))]
    UnsupportedFontFile { path: PathBuf },
    #[snafu(display(
        concat_line!(
            "failed to remove unsupported font file: {path}",
            "manual cleanup may be required",
        ),
        path = path.display(),
    ))]
    RemoveUnsupportedFontFile {
        path: PathBuf,
        source: fs_util::FsError,
    },
}

#[derive(Debug, Snafu)]
enum ValidationErrorReport {
    #[snafu(display("failed to create font validator"))]
    CreateValidator { source: FontValidatorError },
    #[snafu(display("failed to validate font file: {file_name}", file_name = file_name.display()))]
    ValidateFont {
        file_name: FileName,
        source: FontValidatorError,
    },
    #[snafu(display("duplicate font name found in package: {title}"))]
    DuplicateFontName { title: String },
}

pub(super) fn validate_and_prune_fonts<S>(
    cx: &ReportContext<S>,
    fonts_dir: &AbsolutePath,
    file_names: &[FileName],
) -> Result<Vec<FontEntry>, S::Error>
where
    S: ReportScope,
{
    let cx = ValidationScope::start_with_report(cx, "Validating fonts...");

    let mut valid_entries = vec![];
    let mut valid_entry_titles = HashSet::new();
    let validator = FontValidator::new()
        .context(CreateValidatorSnafu)
        .report_error(&cx)?;

    for file_name in file_names {
        let Some(entry) = validator
            .validate_font(fonts_dir, file_name)
            .context(ValidateFontSnafu { file_name })
            .report_error(&cx)?
        else {
            let path = fonts_dir.join(file_name);
            UnsupportedFontFileSnafu { path: &path }
                .build()
                .report_warn(&cx)?;
            fs_util::remove_file(&path)
                .context(RemoveUnsupportedFontFileSnafu { path: &path })
                .report_warn(&cx)?;
            continue;
        };

        if !valid_entry_titles.insert(entry.title().to_lowercase()) {
            return Err(DuplicateFontNameSnafu {
                title: entry.title(),
            }
            .build()
            .report_error(&cx));
        }

        valid_entries.push(entry);
    }

    Ok(valid_entries)
}
