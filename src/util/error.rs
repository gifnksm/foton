use std::fmt::{self, Display, Formatter};

use crate::cli::message::{error, warn};

pub(crate) trait FormatErrorChain {
    fn format_error_chain(&self) -> impl Display + '_;
}

impl<E> FormatErrorChain for E
where
    E: std::error::Error + ?Sized,
{
    fn format_error_chain(&self) -> impl Display + '_ {
        ErrorChainDisplay { error: self }
    }
}

struct ErrorChainDisplay<'a, E>
where
    E: ?Sized,
{
    error: &'a E,
}

impl<E> Display for ErrorChainDisplay<'_, E>
where
    E: std::error::Error + ?Sized,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.error)?;

        let mut source = self.error.source();
        while let Some(err) = source {
            write!(f, "\n  caused by: {err}")?;
            source = err.source();
        }

        Ok(())
    }
}

pub(crate) trait IgnoreNotFound {
    fn ignore_not_found(self) -> Self;
}

impl IgnoreNotFound for std::io::Result<()> {
    fn ignore_not_found(self) -> Self {
        match self {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }
}

pub(crate) trait IgnoreError {
    fn ignore_err_with_error(self);
    fn ignore_err_with_warn(self);
}

impl<E> IgnoreError for Result<(), E>
where
    E: AsRef<dyn std::error::Error>,
{
    fn ignore_err_with_error(self) {
        if let Err(err) = self {
            error!("{}", err.as_ref().format_error_chain());
        }
    }

    fn ignore_err_with_warn(self) {
        if let Err(err) = self {
            warn!("{}", err.as_ref().format_error_chain());
        }
    }
}

pub(crate) trait MessageResultExt {
    type Item;

    fn ok_with_warn(self) -> Option<Self::Item>;
}

impl<T, E> MessageResultExt for Result<T, E>
where
    E: AsRef<dyn std::error::Error>,
{
    type Item = T;

    fn ok_with_warn(self) -> Option<Self::Item> {
        self.inspect_err(|err| warn!("{}", err.as_ref().format_error_chain()))
            .ok()
    }
}
