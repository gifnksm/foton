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

macro_rules! _message_notice {
    ($($arg:tt)*) => {
        $crate::cli::message::eprintln_notice(::std::format_args!($($arg)*))
    };
}

macro_rules! _message_warn {
    ($($arg:tt)*) => {
        $crate::cli::message::eprintln_warn(::std::format_args!($($arg)*))
    };
}

macro_rules! _message_error {
    ($($arg:tt)*) => {
        $crate::cli::message::eprintln_error(::std::format_args!($($arg)*))
    };
}

pub(crate) use _message_error as error;
pub(crate) use _message_info as info;
pub(crate) use _message_notice as notice;
pub(crate) use _message_scope as scope;
pub(crate) use _message_warn as warn;
use console::Style;

struct MessageStyle {
    prefix_style: Style,
    body_style: Style,
    head_prefix: &'static str,
    tail_prefix: &'static str,
    head_separator: &'static str,
    tail_separator: &'static str,
}

impl MessageStyle {
    fn eprint(&self, message: fmt::Arguments<'_>) {
        let message = message.to_string();
        let mut message = message.lines();
        let line = message.next().unwrap_or("");
        eprintln!(
            "{}{}{}",
            self.prefix_style.apply_to(self.head_prefix),
            self.head_separator,
            self.body_style.apply_to(line),
        );
        for line in message {
            eprintln!(
                "{}{}{}",
                self.prefix_style.apply_to(self.tail_prefix),
                self.tail_separator,
                self.body_style.apply_to(line),
            );
        }
    }
}

const SCOPE_STYLE: MessageStyle = MessageStyle {
    prefix_style: Style::new().blue().bold().bright(),
    body_style: Style::new().bold().bright(),
    head_prefix: "::",
    tail_prefix: "  ",
    head_separator: " ",
    tail_separator: " ",
};
const NOTICE_STYLE: MessageStyle = MessageStyle {
    prefix_style: Style::new().blue().bold().bright(),
    body_style: Style::new().bold().bright(),
    head_prefix: "notice",
    tail_prefix: "      ",
    head_separator: ": ",
    tail_separator: "  ",
};
const WARNING_STYLE: MessageStyle = MessageStyle {
    prefix_style: Style::new().yellow().bold().bright(),
    body_style: Style::new().bold().bright(),
    head_prefix: "warning",
    tail_prefix: "       ",
    head_separator: ": ",
    tail_separator: "  ",
};
const ERROR_STYLE: MessageStyle = MessageStyle {
    prefix_style: Style::new().red().bold().bright(),
    body_style: Style::new().bold().bright(),
    head_prefix: "error",
    tail_prefix: "     ",
    head_separator: ": ",
    tail_separator: "  ",
};

pub(crate) fn eprintln_scope(message: fmt::Arguments<'_>) {
    SCOPE_STYLE.eprint(message);
}

pub(crate) fn eprintln_info(message: fmt::Arguments<'_>) {
    eprintln!("{message}");
}

pub(crate) fn eprintln_notice(message: fmt::Arguments<'_>) {
    NOTICE_STYLE.eprint(message);
}

pub(crate) fn eprintln_warn(message: fmt::Arguments<'_>) {
    WARNING_STYLE.eprint(message);
}

pub(crate) fn eprintln_error(message: fmt::Arguments<'_>) {
    ERROR_STYLE.eprint(message);
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
