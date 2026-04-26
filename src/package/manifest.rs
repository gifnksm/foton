use reqwest::Url;
use semver::Version;
use serde::{Deserialize, Serialize};

use crate::{
    package::{PackageId, PackageQualifiedName},
    util::hash::GenericDigest,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PackageManifest {
    #[serde(rename = "package")]
    pub(crate) metadata: PackageMetadata,
    #[serde(deserialize_with = "non_empty_vec::deserialize")]
    pub(crate) sources: Vec<PackageSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PackageMetadata {
    #[serde(rename = "name")]
    pub(crate) qualified_name: PackageQualifiedName,
    pub(crate) version: Version,
    #[serde(
        default,
        deserialize_with = "option_nonempty_string_without_surrounding_whitespaces::deserialize"
    )]
    pub(crate) description: Option<String>,
    #[serde(default, deserialize_with = "optional_http_url::deserialize")]
    pub(crate) homepage: Option<Url>,
    #[serde(default, deserialize_with = "optional_http_url::deserialize")]
    pub(crate) repository: Option<Url>,
    #[serde(default, with = "optional_spdx_expression")]
    pub(crate) license: Option<spdx::Expression>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PackageSource {
    #[serde(deserialize_with = "http_url::deserialize")]
    pub(crate) url: Url,
    pub(crate) hash: GenericDigest,
    #[serde(default = "default_include", with = "glob_pattern")]
    pub(crate) include: Vec<glob::Pattern>,
}

impl PackageMetadata {
    pub(crate) fn id(&self) -> PackageId {
        PackageId::new(self.qualified_name.clone(), self.version.clone())
    }
}

fn default_include() -> Vec<glob::Pattern> {
    vec![
        glob::Pattern::new("**/*.ttf").unwrap(),
        glob::Pattern::new("**/*.otf").unwrap(),
        glob::Pattern::new("**/*.ttc").unwrap(),
    ]
}

mod option_nonempty_string_without_surrounding_whitespaces {
    use serde::Deserialize as _;

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let opt_str: Option<String> = Option::deserialize(deserializer)?;
        if let Some(ref s) = opt_str
            && let t = s.trim()
            && (t.is_empty() || t != s)
        {
            return Err(serde::de::Error::invalid_value(
                serde::de::Unexpected::Str(s),
                &"a non-empty string without leading or trailing whitespace",
            ));
        }
        Ok(opt_str)
    }
}

mod non_empty_vec {
    use serde::Deserialize;

    pub(super) fn deserialize<'de, T, D>(deserializer: D) -> Result<Vec<T>, D::Error>
    where
        T: Deserialize<'de>,
        D: serde::Deserializer<'de>,
    {
        let vec: Vec<T> = Vec::deserialize(deserializer)?;
        if vec.is_empty() {
            return Err(serde::de::Error::invalid_value(
                serde::de::Unexpected::Seq,
                &"a non-empty array",
            ));
        }
        Ok(vec)
    }
}

mod optional_http_url {
    use serde::Deserialize as _;
    use url::Url;

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<Option<Url>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let opt_url: Option<Url> = Option::deserialize(deserializer)?;
        if let Some(ref url) = opt_url
            && url.scheme() != "http"
            && url.scheme() != "https"
        {
            return Err(serde::de::Error::invalid_value(
                serde::de::Unexpected::Str(url.as_str()),
                &"a URL with http or https scheme",
            ));
        }
        Ok(opt_url)
    }
}

mod optional_spdx_expression {
    use std::string::ToString;

    use serde::{Deserialize as _, Serialize as _};
    use spdx::Expression;

    #[expect(clippy::ref_option)]
    pub(super) fn serialize<S>(expr: &Option<Expression>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        expr.as_ref().map(ToString::to_string).serialize(serializer)
    }

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<Option<Expression>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let expr: Option<String> = Option::deserialize(deserializer)?;
        expr.map(|s| s.parse::<Expression>())
            .transpose()
            .map_err(|e| serde::de::Error::custom(format!("invalid SPDX expression: {e}")))
    }
}

mod http_url {
    use serde::Deserialize as _;
    use url::Url;

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<Url, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let url: Url = Url::deserialize(deserializer)?;
        if url.scheme() != "http" && url.scheme() != "https" {
            return Err(serde::de::Error::invalid_value(
                serde::de::Unexpected::Str(url.as_str()),
                &"a URL with http or https scheme",
            ));
        }
        Ok(url)
    }
}

mod glob_pattern {
    use serde::{Deserialize as _, Serialize as _};

    pub(super) fn serialize<S>(patterns: &[glob::Pattern], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let pattern_strs: Vec<String> = patterns.iter().map(|p| p.as_str().to_string()).collect();
        pattern_strs.serialize(serializer)
    }

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<Vec<glob::Pattern>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let pattern_strs: Vec<String> = Vec::deserialize(deserializer)?;
        if pattern_strs.is_empty() {
            return Err(serde::de::Error::invalid_value(
                serde::de::Unexpected::Seq,
                &"a non-empty array",
            ));
        }
        let patterns = pattern_strs
            .into_iter()
            .map(|s| glob::Pattern::new(&s).map_err(serde::de::Error::custom))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(patterns)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_manifest(input: &str) -> Result<PackageManifest, toml::de::Error> {
        toml::from_str(input)
    }

    fn valid_manifest_toml() -> &'static str {
        r#"
[package]
name = "yuru7/hackgen"
version = "2.10.0"
description = "HackGen"
homepage = "https://github.com/yuru7/HackGen"
repository = "https://github.com/yuru7/HackGen"
license = "OFL-1.1"

[[sources]]
url = "https://github.com/yuru7/HackGen/releases/download/v2.10.0/HackGen_v2.10.0.zip"
hash = "sha256:ed182e2a4b95792d94dea7932f6b45280b5ae353651be249d5f6b7867b788db7"
include = ["*/*.ttf"]
"#
    }

    #[test]
    fn package_manifest_deserializes_valid_manifest() {
        let manifest = parse_manifest(valid_manifest_toml()).unwrap();

        assert_eq!(manifest.metadata.qualified_name.namespace(), "yuru7");
        assert_eq!(manifest.metadata.qualified_name.name(), "hackgen");
        assert_eq!(manifest.metadata.version, Version::new(2, 10, 0));
        assert_eq!(manifest.metadata.description.as_deref(), Some("HackGen"));
        assert_eq!(
            manifest.metadata.homepage.as_ref().map(Url::as_str),
            Some("https://github.com/yuru7/HackGen")
        );
        assert_eq!(
            manifest.metadata.repository.as_ref().map(Url::as_str),
            Some("https://github.com/yuru7/HackGen")
        );
        assert_eq!(
            manifest.metadata.license.as_ref().map(ToString::to_string),
            Some("OFL-1.1".to_string())
        );
        assert_eq!(manifest.sources.len(), 1);
        assert_eq!(
            manifest.sources[0].url.as_str(),
            "https://github.com/yuru7/HackGen/releases/download/v2.10.0/HackGen_v2.10.0.zip"
        );
        assert_eq!(
            manifest.sources[0].hash.to_string(),
            "sha256:ed182e2a4b95792d94dea7932f6b45280b5ae353651be249d5f6b7867b788db7"
        );
        assert_eq!(
            manifest.sources[0]
                .include
                .iter()
                .map(glob::Pattern::as_str)
                .collect::<Vec<_>>(),
            vec!["*/*.ttf"]
        );
    }

    #[test]
    fn package_manifest_rejects_invalid_license_expression() {
        let err = parse_manifest(
            r#"
[package]
name = "yuru7/hackgen"
version = "2.10.0"
license = "not-a-valid-spdx"

[[sources]]
url = "https://github.com/yuru7/HackGen/releases/download/v2.10.0/HackGen_v2.10.0.zip"
hash = "sha256:ed182e2a4b95792d94dea7932f6b45280b5ae353651be249d5f6b7867b788db7"
"#,
        )
        .unwrap_err();

        assert!(err.to_string().contains("invalid SPDX expression"));
    }

    #[derive(Debug, Deserialize)]
    struct NonEmptyVecWrapper {
        #[serde(deserialize_with = "super::non_empty_vec::deserialize")]
        values: Vec<u32>,
    }

    #[test]
    fn non_empty_vec_deserializer_accepts_non_empty_array() {
        let wrapper: NonEmptyVecWrapper = toml::from_str("values = [1]").unwrap();

        assert_eq!(wrapper.values, vec![1]);
    }

    #[test]
    fn non_empty_vec_deserializer_rejects_empty_array() {
        // `PackageManifest` uses `[[sources]]` array-of-tables syntax, so `sources = []`
        // fails as a TOML shape mismatch before the custom deserializer runs.
        // Test the helper directly with a minimal wrapper instead.
        let err = toml::from_str::<NonEmptyVecWrapper>("values = []").unwrap_err();

        assert!(err.to_string().contains("a non-empty array"));
    }

    #[test]
    fn package_manifest_requires_sources_as_array_of_tables() {
        // `sources = []` is not the same TOML shape as `[[sources]]`.
        // This fails during manifest deserialization before the custom non-empty
        // vector deserializer runs, so keep a separate helper test above for the
        // actual non-empty validation behavior.
        let err = parse_manifest(
            r#"
[package]
name = "yuru7/hackgen"
version = "2.10.0"

sources = []
"#,
        )
        .unwrap_err();

        assert!(err.to_string().contains("missing field `sources`"));
    }

    #[test]
    fn package_manifest_rejects_empty_include() {
        let err = parse_manifest(
            r#"
[package]
name = "yuru7/hackgen"
version = "2.10.0"

[[sources]]
url = "https://github.com/yuru7/HackGen/releases/download/v2.10.0/HackGen_v2.10.0.zip"
hash = "sha256:ed182e2a4b95792d94dea7932f6b45280b5ae353651be249d5f6b7867b788db7"
include = []
"#,
        )
        .unwrap_err();

        assert!(err.to_string().contains("a non-empty array"));
    }

    #[test]
    fn package_manifest_rejects_empty_description() {
        let err = parse_manifest(
            r#"
[package]
name = "yuru7/hackgen"
version = "2.10.0"
description = ""

[[sources]]
url = "https://github.com/yuru7/HackGen/releases/download/v2.10.0/HackGen_v2.10.0.zip"
hash = "sha256:ed182e2a4b95792d94dea7932f6b45280b5ae353651be249d5f6b7867b788db7"
"#,
        )
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("a non-empty string without leading or trailing whitespace")
        );
    }

    #[test]
    fn package_manifest_rejects_description_with_surrounding_whitespace() {
        let err = parse_manifest(
            r#"
[package]
name = "yuru7/hackgen"
version = "2.10.0"
description = " HackGen "

[[sources]]
url = "https://github.com/yuru7/HackGen/releases/download/v2.10.0/HackGen_v2.10.0.zip"
hash = "sha256:ed182e2a4b95792d94dea7932f6b45280b5ae353651be249d5f6b7867b788db7"
"#,
        )
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("a non-empty string without leading or trailing whitespace")
        );
    }

    #[test]
    fn package_manifest_rejects_non_http_source_url() {
        let err = parse_manifest(
            r#"
[package]
name = "yuru7/hackgen"
version = "2.10.0"

[[sources]]
url = "file:///tmp/HackGen_v2.10.0.zip"
hash = "sha256:ed182e2a4b95792d94dea7932f6b45280b5ae353651be249d5f6b7867b788db7"
"#,
        )
        .unwrap_err();

        assert!(err.to_string().contains("a URL with http or https scheme"));
    }

    #[test]
    fn package_manifest_rejects_non_http_homepage_url() {
        let err = parse_manifest(
            r#"
[package]
name = "yuru7/hackgen"
version = "2.10.0"
homepage = "file:///tmp/project"

[[sources]]
url = "https://github.com/yuru7/HackGen/releases/download/v2.10.0/HackGen_v2.10.0.zip"
hash = "sha256:ed182e2a4b95792d94dea7932f6b45280b5ae353651be249d5f6b7867b788db7"
"#,
        )
        .unwrap_err();

        assert!(err.to_string().contains("a URL with http or https scheme"));
    }

    #[test]
    fn package_manifest_uses_default_include_when_omitted() {
        let manifest = parse_manifest(
            r#"
[package]
name = "yuru7/hackgen"
version = "2.10.0"

[[sources]]
url = "https://github.com/yuru7/HackGen/releases/download/v2.10.0/HackGen_v2.10.0.zip"
hash = "sha256:ed182e2a4b95792d94dea7932f6b45280b5ae353651be249d5f6b7867b788db7"
"#,
        )
        .unwrap();

        assert_eq!(
            manifest.sources[0]
                .include
                .iter()
                .map(glob::Pattern::as_str)
                .collect::<Vec<_>>(),
            vec!["**/*.ttf", "**/*.otf", "**/*.ttc"]
        );
    }
}
