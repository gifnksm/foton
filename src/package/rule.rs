use std::fmt::{self, Display};

use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, value::MapAccessDeserializer},
};

use crate::{
    package::manifest,
    util::{
        glob::PathGlob,
        path_matcher::{PathMatcher, PathMatcherError},
    },
};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(into = "RawFileRule")]
pub(crate) struct FileRule {
    matcher: PathMatcher,
    title: Option<String>,
}

impl FileRule {
    #[cfg(test)]
    pub(crate) fn path<P>(path: P) -> Result<Self, PathRuleError>
    where
        P: Into<String>,
    {
        Ok(Self {
            matcher: PathMatcher::path(path)?,
            title: None,
        })
    }

    pub(crate) fn glob(glob: PathGlob) -> Self {
        Self {
            matcher: PathMatcher::glob(glob),
            title: None,
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

    pub(crate) fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }
}

impl Display for FileRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.matcher, f)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(into = "RawIgnoreRule")]
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
            matcher: PathMatcher::path(path)?,
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
    #[display("`title` may be specified only together with `path`")]
    TitleWithoutPath,
}

impl From<PathMatcherError> for PathRuleError {
    fn from(value: PathMatcherError) -> Self {
        match value {
            PathMatcherError::EmptyPath => Self::EmptyPath,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFileRule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    glob: Option<PathGlob>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "manifest::option_nonempty_string_without_surrounding_whitespaces::deserialize"
    )]
    title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawIgnoreRule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    glob: Option<PathGlob>,
}

impl<'de> Deserialize<'de> for FileRule {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> de::Visitor<'de> for Visitor {
            type Value = FileRule;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "a path string or an object with `path` / `glob`")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(FileRule {
                    matcher: PathMatcher::path(value.to_owned()).map_err(E::custom)?,
                    title: None,
                })
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(FileRule {
                    matcher: PathMatcher::path(value).map_err(E::custom)?,
                    title: None,
                })
            }

            fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
            where
                A: de::MapAccess<'de>,
            {
                let raw = RawFileRule::deserialize(MapAccessDeserializer::new(map))?;
                FileRule::try_from(raw).map_err(de::Error::custom)
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

impl<'de> Deserialize<'de> for IgnoreRule {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> de::Visitor<'de> for Visitor {
            type Value = IgnoreRule;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "a path string or an object with `path` / `glob`")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(IgnoreRule {
                    matcher: PathMatcher::path(value.to_owned()).map_err(E::custom)?,
                })
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(IgnoreRule {
                    matcher: PathMatcher::path(value).map_err(E::custom)?,
                })
            }

            fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
            where
                A: de::MapAccess<'de>,
            {
                let raw = RawIgnoreRule::deserialize(MapAccessDeserializer::new(map))?;
                IgnoreRule::try_from(raw).map_err(de::Error::custom)
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

impl TryFrom<RawFileRule> for FileRule {
    type Error = PathRuleError;

    fn try_from(value: RawFileRule) -> Result<Self, Self::Error> {
        let RawFileRule { path, glob, title } = value;
        let (matcher, title) = match (path, glob, title) {
            (Some(path), None, title) => (PathMatcher::path(path)?, title),
            (None, Some(glob), None) => (PathMatcher::glob(glob), None),
            (None, Some(_), Some(_)) => return Err(PathRuleError::TitleWithoutPath),
            (Some(_), Some(_), _) | (None, None, _) => {
                return Err(PathRuleError::InvalidMatcher);
            }
        };
        Ok(Self { matcher, title })
    }
}

impl From<FileRule> for RawFileRule {
    fn from(value: FileRule) -> Self {
        let FileRule { matcher, title } = value;
        match matcher {
            PathMatcher::Path(path) => Self {
                path: Some(path.into_original()),
                glob: None,
                title,
            },
            PathMatcher::Glob(glob) => Self {
                path: None,
                glob: Some(glob),
                title: None,
            },
        }
    }
}

impl TryFrom<RawIgnoreRule> for IgnoreRule {
    type Error = PathRuleError;

    fn try_from(value: RawIgnoreRule) -> Result<Self, Self::Error> {
        let RawIgnoreRule { path, glob } = value;
        let matcher = match (path, glob) {
            (Some(path), None) => PathMatcher::path(path)?,
            (None, Some(glob)) => PathMatcher::glob(glob),
            (Some(_), Some(_)) | (None, None) => return Err(PathRuleError::InvalidMatcher),
        };
        Ok(Self { matcher })
    }
}

impl From<IgnoreRule> for RawIgnoreRule {
    fn from(value: IgnoreRule) -> Self {
        let IgnoreRule { matcher } = value;
        match matcher {
            PathMatcher::Path(path) => Self {
                path: Some(path.into_original()),
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
        let err = toml::from_str::<FileRule>(
            "path = \"fonts/A.ttf\"\n__foton_unused_field__ = \"B.ttf\"",
        )
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("unknown field `__foton_unused_field__`"),
            "{}",
            err.to_string()
        );
    }

    #[test]
    fn file_rule_rejects_path_and_glob_together() {
        let err = toml::from_str::<FileRule>("path = \"fonts/A.ttf\"\nglob = \"fonts/*.ttf\"")
            .unwrap_err();

        assert!(
            err.to_string()
                .contains("exactly one of `path` or `glob` must be specified"),
            "{}",
            err.to_string()
        );
    }

    #[test]
    fn file_rule_rejects_title_without_path() {
        let err = toml::from_str::<FileRule>("glob = \"fonts/*.ttf\"\ntitle = \"Example Font\"")
            .unwrap_err();

        assert!(
            err.to_string()
                .contains("`title` may be specified only together with `path`"),
            "{}",
            err.to_string()
        );
    }

    #[test]
    fn ignore_rule_rejects_unknown_fields() {
        let err = toml::from_str::<IgnoreRule>(
            "path = \"fonts/A.ttf\"\n__foton_unused_field__ = \"B.ttf\"",
        )
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("unknown field `__foton_unused_field__`"),
            "{}",
            err.to_string()
        );
    }

    #[test]
    fn ignore_rule_rejects_path_and_glob_together() {
        let err = toml::from_str::<IgnoreRule>("path = \"fonts/A.ttf\"\nglob = \"fonts/*.ttf\"")
            .unwrap_err();

        assert!(
            err.to_string()
                .contains("exactly one of `path` or `glob` must be specified"),
            "{}",
            err.to_string()
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
