use std::fmt;

macro_rules! _message_step {
    ($($arg:tt)*) => {
        $crate::cli::message::eprintln_step(::std::format_args!($($arg)*))
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

use console::Style;

pub(crate) use {
    _message_error as error, _message_info as info, _message_step as step, _message_warn as warn,
};

const STEP_PREFIX_STYLE: Style = Style::new().blue().bold();
const STEP_BODY_STYLE: Style = Style::new().bold();
const ERROR_PREFIX_STYLE: Style = Style::new().red().bold();
const WARNING_PREFIX_STYLE: Style = Style::new().yellow().bold();

pub(crate) fn eprintln_step(message: fmt::Arguments<'_>) {
    eprintln!(
        "{} {}",
        STEP_PREFIX_STYLE.apply_to("::"),
        STEP_BODY_STYLE.apply_to(message)
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
