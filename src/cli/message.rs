use std::fmt::{self, Display};

macro_rules! _message_scope {
    ($($arg:tt)*) => {
        $crate::cli::message::eprintln_scope(::std::format_args!($($arg)*))
    };
}

macro_rules! _message_info {
    ($($arg:tt)*) => {
        $crate::cli::message::eprintln_info(::std::format_args!($($arg)*))
    };
}

macro_rules! _message_error {
    ($($arg:tt)*) => {
        $crate::cli::message::eprintln_error(::std::format_args!($($arg)*))
    };
}

macro_rules! _message_warn {
    ($($arg:tt)*) => {
        $crate::cli::message::eprintln_warn(::std::format_args!($($arg)*))
    };
}

pub(crate) use _message_error as error;
pub(crate) use _message_info as info;
pub(crate) use _message_scope as scope;
pub(crate) use _message_warn as warn;
use console::Style;

const SCOPE_PREFIX_STYLE: Style = Style::new().blue().bold();
const SCOPE_BODY_STYLE: Style = Style::new().bold();
const ERROR_PREFIX_STYLE: Style = Style::new().red().bold();
const WARNING_PREFIX_STYLE: Style = Style::new().yellow().bold();

pub(crate) fn eprintln_scope(message: fmt::Arguments<'_>) {
    eprintln!(
        "{} {}",
        SCOPE_PREFIX_STYLE.apply_to("::"),
        SCOPE_BODY_STYLE.apply_to(message)
    );
}

pub(crate) fn eprintln_info(message: fmt::Arguments<'_>) {
    eprintln!("{message}");
}

pub(crate) fn eprintln_error(message: fmt::Arguments<'_>) {
    eprintln!("{}: {message}", ERROR_PREFIX_STYLE.apply_to("error"));
}

pub(crate) fn eprintln_warn(message: fmt::Arguments<'_>) {
    eprintln!("{}: {message}", WARNING_PREFIX_STYLE.apply_to("warning"));
}

#[derive(Debug)]
pub(crate) struct BulletList<'a, I>(pub(crate) &'a I);

impl<'a, I> Display for BulletList<'a, I>
where
    &'a I: IntoIterator,
    <&'a I as IntoIterator>::Item: Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, item) in self.0.into_iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            write!(f, "  - {item}")?;
        }
        Ok(())
    }
}
