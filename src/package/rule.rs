use std::fmt::{self, Display};

use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, value::MapAccessDeserializer},
};

use crate::util::{
    glob::PathGlob,
    path::FileName,
    path_matcher::{PathMatcher, PathMatcherError},
};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(into = "RawFileRule")]
#[serde(rename_all = "kebab-case")]
pub(crate) struct FontRule {
    matcher: PathMatcher,
    file_name: Option<FileName>,
}

impl FontRule {
    #[cfg(test)]
    pub(crate) fn path<P>(path: P) -> Result<Self, PathRuleError>
    where
        P: Into<String>,
    {
        Ok(Self {
            matcher: PathMatcher::path(path)?,
            file_name: None,
        })
    }

    pub(crate) fn glob(glob: PathGlob) -> Self {
        Self {
            matcher: PathMatcher::glob(glob),
            file_name: None,
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

    pub(crate) fn file_name(&self) -> Option<&FileName> {
        self.file_name.as_ref()
    }
}

impl Display for FontRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.matcher, f)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(into = "RawIgnoreRule")]
#[serde(rename_all = "kebab-case")]
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
    #[display("`file-name` may be specified only together with `path`")]
    FileNameWithoutPath,
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
#[serde(rename_all = "kebab-case")]
struct RawFileRule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    glob: Option<PathGlob>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    file_name: Option<FileName>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "kebab-case")]
struct RawIgnoreRule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    glob: Option<PathGlob>,
}

impl<'de> Deserialize<'de> for FontRule {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> de::Visitor<'de> for Visitor {
            type Value = FontRule;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "a path string or an object with `path` / `glob`")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(FontRule {
                    matcher: PathMatcher::path(value.to_owned()).map_err(E::custom)?,
                    file_name: None,
                })
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(FontRule {
                    matcher: PathMatcher::path(value).map_err(E::custom)?,
                    file_name: None,
                })
            }

            fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
            where
                A: de::MapAccess<'de>,
            {
                let raw = RawFileRule::deserialize(MapAccessDeserializer::new(map))?;
                FontRule::try_from(raw).map_err(de::Error::custom)
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

impl TryFrom<RawFileRule> for FontRule {
    type Error = PathRuleError;

    fn try_from(value: RawFileRule) -> Result<Self, Self::Error> {
        let RawFileRule {
            path,
            glob,
            file_name,
        } = value;
        let (matcher, file_name) = match (path, glob, file_name) {
            (Some(path), None, file_name) => (PathMatcher::path(path)?, file_name),
            (None, Some(glob), None) => (PathMatcher::glob(glob), None),
            (None, Some(_), Some(_)) => return Err(PathRuleError::FileNameWithoutPath),
            (Some(_), Some(_), _) | (None, None, _) => return Err(PathRuleError::InvalidMatcher),
        };
        Ok(Self { matcher, file_name })
    }
}

impl From<FontRule> for RawFileRule {
    fn from(value: FontRule) -> Self {
        let FontRule { matcher, file_name } = value;
        match matcher {
            PathMatcher::Path(path) => Self {
                path: Some(path.into_original()),
                glob: None,
                file_name,
            },
            PathMatcher::Glob(glob) => Self {
                path: None,
                glob: Some(glob),
                file_name: None,
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
        struct Proxy {
            value: FontRule,
        }

        let wrapper = toml::from_str::<Proxy>(indoc::indoc! {r#"
            value = "fonts/A.ttf"
        "#})
        .unwrap();

        assert_eq!(
            wrapper.value.matcher(),
            &PathMatcher::path("fonts/A.ttf").unwrap()
        );
    }

    #[test]
    fn file_rule_deserializes_path_object() {
        let rule = toml::from_str::<FontRule>(indoc::indoc! {r#"
            path = "fonts/A.ttf"
        "#})
        .unwrap();

        assert_eq!(rule.matcher(), &PathMatcher::path("fonts/A.ttf").unwrap());
    }

    #[test]
    fn file_rule_deserializes_glob_object() {
        let rule = toml::from_str::<FontRule>(indoc::indoc! {r#"
            glob = "**/*.ttf"
        "#})
        .unwrap();

        assert_eq!(
            rule.matcher(),
            &PathMatcher::Glob(PathGlob::new("**/*.ttf").unwrap())
        );
    }

    #[test]
    fn file_rule_rejects_unknown_fields() {
        let err = toml::from_str::<FontRule>(indoc::indoc! {r#"
            path = "fonts/A.ttf"
            __foton_unused_field__ = "B.ttf"
        "#})
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
        let err = toml::from_str::<FontRule>(indoc::indoc! {r#"
            path = "fonts/A.ttf"
            glob = "fonts/*.ttf"
        "#})
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("exactly one of `path` or `glob` must be specified"),
            "{}",
            err.to_string()
        );
    }

    #[test]
    fn ignore_rule_rejects_unknown_fields() {
        let err = toml::from_str::<IgnoreRule>(indoc::indoc! {r#"
            path = "fonts/A.ttf"
            __foton_unused_field__ = "B.ttf"
        "#})
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
        let err = toml::from_str::<IgnoreRule>(indoc::indoc! {r#"
            path = "fonts/A.ttf"
            glob = "fonts/*.ttf"
        "#})
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
        let rule = FontRule::glob(PathGlob::new("**/*.ttf").unwrap());

        assert_eq!(
            rule.matcher(),
            &PathMatcher::Glob(PathGlob::new("**/*.ttf").unwrap())
        );
    }
}
