macro_rules! _concat_line {
    () => {
        concat!()
    };
    ($arg:expr $(,)?) => {
        concat!($arg)
    };
    ($arg:expr, $($rest:expr),+ $(,)?) => {
        concat!($arg, "\n", $crate::util::macros::concat_line!($($rest),+))
    };
}

pub(crate) use _concat_line as concat_line;
