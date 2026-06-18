use std::fmt::{self, Display};

use serde::{Deserialize, Serialize};

use crate::util::{
    glob::PathGlob,
    path_matcher::{PathMatcher, PathMatcherError},
};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "RawFileRule", into = "RawFileRule")]
pub(crate) struct FileRule {
    matcher: PathMatcher,
}

impl FileRule {
    #[cfg(test)]
    pub(crate) fn path<S>(path: S) -> Result<Self, PathRuleError>
    where
        S: Into<String>,
    {
        Ok(Self {
            matcher: PathMatcher::path(path).map_err(PathRuleError::from)?,
        })
    }

    pub(crate) fn glob(glob: PathGlob) -> Self {
        Self {
            matcher: PathMatcher::glob(glob),
        }
    }

    pub(crate) fn matches<P>(&self, path: P) -> bool
    where
        P: AsRef<std::path::Path>,
    {
        self.matcher.matches(path)
    }

    pub(crate) fn matcher(&self) -> &PathMatcher {
        &self.matcher
    }
}

impl Display for FileRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.matcher, f)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "RawIgnoreRule", into = "RawIgnoreRule")]
pub(crate) struct IgnoreRule {
    matcher: PathMatcher,
}

impl IgnoreRule {
    #[cfg(test)]
    pub(crate) fn path<S>(path: S) -> Result<Self, PathRuleError>
    where
        S: Into<String>,
    {
        Ok(Self {
            matcher: PathMatcher::path(path).map_err(PathRuleError::from)?,
        })
    }

    pub(crate) fn matches<P>(&self, path: P) -> bool
    where
        P: AsRef<std::path::Path>,
    {
        self.matcher.matches(path)
    }
}

impl Display for IgnoreRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.matcher, f)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, derive_more::Display)]
pub(crate) enum PathRuleError {
    #[display("path must be non-empty")]
    EmptyPath,
    #[display("exactly one of `path` or `glob` must be specified")]
    InvalidMatcher,
}

impl From<PathMatcherError> for PathRuleError {
    fn from(value: PathMatcherError) -> Self {
        match value {
            PathMatcherError::EmptyPath => Self::EmptyPath,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
enum RawFileRule {
    Path(String),
    Object(RawFileRuleObject),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
enum RawIgnoreRule {
    Path(String),
    Object(RawIgnoreRuleObject),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFileRuleObject {
    #[serde(flatten)]
    matcher: RawMatcherFields,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawIgnoreRuleObject {
    #[serde(flatten)]
    matcher: RawMatcherFields,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawMatcherFields {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    glob: Option<PathGlob>,
}

impl TryFrom<RawFileRule> for FileRule {
    type Error = PathRuleError;

    fn try_from(value: RawFileRule) -> Result<Self, Self::Error> {
        let matcher = match value {
            RawFileRule::Path(path) => PathMatcher::path(path).map_err(PathRuleError::from)?,
            RawFileRule::Object(RawFileRuleObject { matcher }) => matcher.try_into()?,
        };
        Ok(Self { matcher })
    }
}

impl From<FileRule> for RawFileRule {
    fn from(value: FileRule) -> Self {
        RawFileRule::Object(RawFileRuleObject {
            matcher: value.matcher.into(),
        })
    }
}

impl TryFrom<RawIgnoreRule> for IgnoreRule {
    type Error = PathRuleError;

    fn try_from(value: RawIgnoreRule) -> Result<Self, Self::Error> {
        let matcher = match value {
            RawIgnoreRule::Path(path) => PathMatcher::path(path).map_err(PathRuleError::from)?,
            RawIgnoreRule::Object(RawIgnoreRuleObject { matcher }) => matcher.try_into()?,
        };
        Ok(Self { matcher })
    }
}

impl From<IgnoreRule> for RawIgnoreRule {
    fn from(value: IgnoreRule) -> Self {
        RawIgnoreRule::Object(RawIgnoreRuleObject {
            matcher: value.matcher.into(),
        })
    }
}

impl TryFrom<RawMatcherFields> for PathMatcher {
    type Error = PathRuleError;

    fn try_from(fields: RawMatcherFields) -> Result<Self, Self::Error> {
        match (fields.path, fields.glob) {
            (Some(path), None) => PathMatcher::path(path).map_err(PathRuleError::from),
            (None, Some(glob)) => Ok(PathMatcher::glob(glob)),
            (Some(_), Some(_)) | (None, None) => Err(PathRuleError::InvalidMatcher),
        }
    }
}

impl From<PathMatcher> for RawMatcherFields {
    fn from(value: PathMatcher) -> Self {
        match value {
            PathMatcher::Path(path) => Self {
                path: Some(path.to_string()),
                glob: None,
            },
            PathMatcher::Glob(glob) => Self {
                path: None,
                glob: Some(glob),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::{glob::PathGlob, path_matcher::PathMatcher};

    #[test]
    fn file_rule_deserializes_path_string() {
        #[derive(Deserialize)]
        struct Wrapper {
            value: FileRule,
        }

        let wrapper: Wrapper = toml::from_str("value = \"fonts/A.ttf\"").unwrap();

        assert_eq!(
            wrapper.value.matcher(),
            &PathMatcher::path("fonts/A.ttf").unwrap()
        );
    }

    #[test]
    fn file_rule_deserializes_path_object() {
        let rule: FileRule = toml::from_str("path = \"fonts/A.ttf\"").unwrap();

        assert_eq!(rule.matcher(), &PathMatcher::path("fonts/A.ttf").unwrap());
    }

    #[test]
    fn file_rule_deserializes_glob_object() {
        let rule: FileRule = toml::from_str("glob = \"**/*.ttf\"").unwrap();

        assert_eq!(
            rule.matcher(),
            &PathMatcher::Glob(PathGlob::new("**/*.ttf").unwrap())
        );
    }

    #[test]
    fn file_rule_rejects_unknown_fields() {
        let err =
            toml::from_str::<FileRule>("path = \"fonts/A.ttf\"\nrename = \"B.ttf\"").unwrap_err();

        assert!(err.to_string().contains("data did not match any variant"));
    }

    #[test]
    fn file_rule_rejects_path_and_glob_together() {
        let err = toml::from_str::<FileRule>("path = \"fonts/A.ttf\"\nglob = \"fonts/*.ttf\"")
            .unwrap_err();

        assert!(
            err.to_string()
                .contains("exactly one of `path` or `glob` must be specified")
        );
    }

    #[test]
    fn file_rule_glob_constructor_builds_glob_rule() {
        let rule = FileRule::glob(PathGlob::new("**/*.ttf").unwrap());

        assert_eq!(
            rule.matcher(),
            &PathMatcher::Glob(PathGlob::new("**/*.ttf").unwrap())
        );
    }
}
