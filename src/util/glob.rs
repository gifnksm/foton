use std::{
    fmt::{self, Display},
    path::Path,
};

use glob::{MatchOptions, Pattern, PatternError};
use serde::{Deserialize, Serialize};

const GLOB_MATCH_OPTIONS: MatchOptions = MatchOptions {
    case_sensitive: false,
    require_literal_separator: true,
    require_literal_leading_dot: true,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct PathGlob {
    pattern: Pattern,
}

impl PathGlob {
    pub(crate) fn new(pattern: &str) -> Result<Self, PatternError> {
        Ok(Self {
            pattern: Pattern::new(pattern)?,
        })
    }

    pub(crate) fn escape(path: &str) -> Self {
        Self::new(&Pattern::escape(path)).unwrap()
    }

    #[cfg(test)]
    pub(crate) fn as_str(&self) -> &str {
        self.pattern.as_str()
    }

    pub(crate) fn matches<P>(&self, path: P) -> bool
    where
        P: AsRef<Path>,
    {
        self.pattern
            .matches_path_with(path.as_ref(), GLOB_MATCH_OPTIONS)
    }
}

impl Display for PathGlob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.pattern, f)
    }
}

impl Serialize for PathGlob {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.pattern.as_str().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PathGlob {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let pattern = String::deserialize(deserializer)?;
        if pattern.is_empty() {
            return Err(serde::de::Error::invalid_value(
                serde::de::Unexpected::Str(&pattern),
                &"a non-empty string",
            ));
        }
        Self::new(&pattern).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn escape_preserves_backslashes() {
        assert_eq!(PathGlob::escape(r"fonts\A.ttf").as_str(), r"fonts\A.ttf");
    }

    #[test]
    fn escape_matches_original_backslash_path() {
        let glob = PathGlob::escape(r"fonts\A.ttf");

        assert!(glob.matches(Path::new(r"fonts\A.ttf")));
    }

    #[cfg(windows)]
    #[test]
    fn escape_follows_windows_separator_matching() {
        let glob = PathGlob::escape(r"fonts\A.ttf");

        assert!(glob.matches(Path::new("fonts/A.ttf")));
    }

    #[test]
    fn escape_escapes_glob_metacharacters() {
        assert_eq!(
            PathGlob::escape("fonts/[wght].ttf").as_str(),
            "fonts/[[]wght[]].ttf"
        );
    }
}
