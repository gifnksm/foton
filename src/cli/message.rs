use std::fmt;

macro_rules! _message_warn {
    ($($arg:tt)*) => {
        #[expect(clippy::used_underscore_items)]
        $crate::cli::message::_warn(::std::format_args!($($arg)*))
    };
}

pub(crate) use _message_warn as warn;

pub(crate) fn _warn(message: fmt::Arguments<'_>) {
    eprintln!("warning: {message}");
}
