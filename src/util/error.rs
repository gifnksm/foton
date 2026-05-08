use std::fmt::{self, Display, Formatter};

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
        if source.is_some() {
            write!(f, "\ncaused by:")?;
        }
        while let Some(err) = source {
            source = err.source();
            let has_next = source.is_some();
            let head_prefix = if has_next { "├─▶" } else { "╰─▶" };
            let tail_prefix = if has_next { "│  " } else { "   " };
            let message = err.to_string();
            let mut lines = message.lines();
            let line = lines.next().unwrap_or("");
            write!(f, "\n  {head_prefix}{line}")?;
            for line in lines {
                write!(f, "\n  {tail_prefix}{line}")?;
            }
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
