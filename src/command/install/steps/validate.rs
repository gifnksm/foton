use std::collections::HashSet;

use crate::{
    package::FontEntry,
    platform::windows::services::font::{FontValidator, FontValidatorError},
    util::{
        fs as fs_util,
        path::{AbsolutePath, FileName},
        reporter::{
            ReportValue, Reporter, Step, StepReporter, StepResultErrorExt as _,
            StepResultWarnExt as _,
        },
    },
};

#[derive(Debug)]
struct ValidationStep<'a, S> {
    step: &'a S,
}

impl<S> Step for ValidationStep<'_, S>
where
    S: Step,
{
    type WarnReportValue = ValidationWarnReport;
    type ErrorReportValue = ValidationErrorReport;
    type Error = S::Error;

    fn report_prelude(&self, reporter: &Reporter) {
        reporter.report_step(format_args!("Validating fonts..."));
    }

    fn make_error(&self) -> Self::Error {
        self.step.make_error()
    }
}

#[derive(Debug, derive_more::Display, derive_more::Error, derive_more::From)]
enum ValidationWarnReport {
    #[display("removing unsupported font file: {path}", path = path.display())]
    RemovingUnsupportedFontFile { path: AbsolutePath },
    #[display("failed to remove unsupported font file: {path}; manual cleanup may be required", path = path.display())]
    RemoveUnsupportedFontFile {
        path: AbsolutePath,
        #[error(source)]
        source: fs_util::FsError,
    },
}

impl From<ValidationWarnReport> for ReportValue<'static> {
    fn from(report: ValidationWarnReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
enum ValidationErrorReport {
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

impl From<ValidationErrorReport> for ReportValue<'static> {
    fn from(report: ValidationErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

pub(in crate::command::install) fn validate_and_prune_fonts<S>(
    reporter: &StepReporter<'_, S>,
    fonts_dir: &AbsolutePath,
    file_names: &[FileName],
) -> Result<Vec<FontEntry>, S::Error>
where
    S: Step,
{
    let reporter = reporter.with_step(ValidationStep {
        step: reporter.step(),
    });

    let mut valid_entries = vec![];
    let mut valid_entry_titles = HashSet::new();
    let validator = FontValidator::new()
        .map_err(|source| ValidationErrorReport::CreateValidator { source })
        .report_error(&reporter)?;

    for file_name in file_names {
        let Some(entry) = validator
            .validate_font(fonts_dir, file_name)
            .map_err(|source| {
                let file_name = file_name.clone();
                ValidationErrorReport::ValidateFont { file_name, source }
            })
            .report_error(&reporter)?
        else {
            let path = fonts_dir.join(file_name);
            reporter.report_warn(ValidationWarnReport::RemovingUnsupportedFontFile {
                path: path.clone(),
            });
            fs_util::remove_file(&path)
                .map_err(|source| ValidationWarnReport::RemoveUnsupportedFontFile { path, source })
                .report_warn(&reporter);
            continue;
        };

        if !valid_entry_titles.insert(entry.title().to_lowercase()) {
            let title = entry.title().to_owned();
            return Err(reporter.report_error(ValidationErrorReport::DuplicateFontName { title }));
        }

        valid_entries.push(entry);
    }

    Ok(valid_entries)
}
