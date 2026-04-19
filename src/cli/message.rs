use std::fmt;

macro_rules! _message_error {
    ($($arg:tt)*) => {
        $crate::cli::message::error(::std::format_args!($($arg)*))
    };
}

macro_rules! _message_warn {
    ($($arg:tt)*) => {
        $crate::cli::message::warn(::std::format_args!($($arg)*))
    };
}

pub(crate) use {_message_error as error, _message_warn as warn};

pub(crate) fn error(message: fmt::Arguments<'_>) {
    eprintln!("error: {message}");
}

pub(crate) fn warn(message: fmt::Arguments<'_>) {
    eprintln!("warning: {message}");
}
