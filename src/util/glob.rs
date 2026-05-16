use glob::{MatchOptions, Pattern};

pub(crate) const GLOB_MATCH_OPTIONS: MatchOptions = MatchOptions {
    case_sensitive: false,
    require_literal_separator: true,
    require_literal_leading_dot: true,
};

pub(crate) fn is_wildcard_pattern(pattern: &Pattern) -> bool {
    pattern.as_str().contains(['*', '?', '['])
}
