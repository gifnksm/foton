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
pub(crate) struct PathPattern {
    pattern: Pattern,
}

impl PathPattern {
    pub(crate) fn new(pattern: &str) -> Result<Self, PatternError> {
        Ok(Self {
            pattern: Pattern::new(pattern)?,
        })
    }

    pub(crate) fn as_str(&self) -> &str {
        self.pattern.as_str()
    }

    pub(crate) fn contains_wildcard(&self) -> bool {
        self.as_str().contains(['*', '?', '['])
    }

    pub(crate) fn matches<P>(&self, path: P) -> bool
    where
        P: AsRef<Path>,
    {
        self.pattern
            .matches_path_with(path.as_ref(), GLOB_MATCH_OPTIONS)
    }
}

impl Display for PathPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.pattern, f)
    }
}

impl Serialize for PathPattern {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.pattern.as_str().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PathPattern {
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
