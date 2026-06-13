use std::{
    fs, io,
    path::{Path, PathBuf},
};

use reqwest::Url;
use serde::{Deserialize, Serialize};
use snafu::{IntoError as _, ResultExt as _, Snafu};

use crate::{
    package::{PackageDefinition, PackageId, PackageName, PackageSource, PackageVersion},
    util::{glob::PathPattern, hash::GenericDigest},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct PackageManifest {
    /// Canonical package name used in package specifiers such as `hackgen`.
    ///
    /// This is the stable identifier for the package and is not intended to match every
    /// user-facing font name exactly.
    pub(crate) name: PackageName,
    /// Human-friendly primary name for the font family, collection, or bundle provided by the
    /// package, such as `HackGen`.
    ///
    /// For packages that primarily provide a single font family, this is usually that family
    /// name. For packages that contain multiple families, use the family, collection, or bundle
    /// name that best represents what users will install.
    ///
    /// Use this for the primary label shown to users in search results and other output.
    #[serde(
        default,
        deserialize_with = "option_nonempty_string_without_surrounding_whitespaces::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) display_name: Option<String>,
    /// Package version.
    ///
    /// This identifies a specific immutable release of the package.
    pub(crate) version: PackageVersion,
    /// Short description of the font family, collection, or bundle provided by the package.
    #[serde(
        default,
        deserialize_with = "option_nonempty_string_without_surrounding_whitespaces::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) description: Option<String>,
    /// Alternative names and spellings for the font family, collection, or bundle used for
    /// search.
    ///
    /// Use this for other names by which users may look for the fonts provided by the package,
    /// such as common alternate spellings, abbreviations, or additional family names included in
    /// a multi-family package.
    ///
    /// Do not use this for names of individual font faces; use `faces` for those.
    #[serde(
        default,
        deserialize_with = "vec_nonempty_strings_without_surrounding_whitespaces::deserialize",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub(crate) aliases: Vec<String>,
    /// Human-friendly entries describing the individual font faces included in the package.
    ///
    /// Use this for face-level entries that distinguish specific variants such as Regular, Bold,
    /// Italic, or similar styles, rather than for package-level or family-level names.
    ///
    /// Each entry is currently written as a face name string, such as `Hackgen Regular` or
    /// `Hackgen Bold`, which helps users find a package by the names shown in font pickers or
    /// Windows shell UI.
    #[serde(
        default,
        deserialize_with = "vec_nonempty_strings_without_surrounding_whitespaces::deserialize",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub(crate) faces: Vec<String>,
    /// Upstream homepage for the font project or distribution represented by the package.
    #[serde(
        default,
        deserialize_with = "optional_http_url::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) homepage: Option<Url>,
    /// Upstream source repository for the font project represented by the package.
    #[serde(
        default,
        deserialize_with = "optional_http_url::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) repository: Option<Url>,
    /// SPDX license expression for the upstream font files included in the package.
    #[serde(
        default,
        with = "optional_spdx_expression",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) license: Option<spdx::Expression>,
    /// Download sources from which the package's font files can be installed.
    #[serde(deserialize_with = "non_empty_vec::deserialize")]
    pub(crate) sources: Vec<PackageManifestSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct PackageManifestSource {
    /// URL of the downloadable archive or file that contains the package contents.
    #[serde(deserialize_with = "http_url::deserialize")]
    pub(crate) url: Url,
    /// Expected digest of the downloaded source used to verify integrity.
    pub(crate) hash: GenericDigest,
    /// Glob patterns selecting which font files to install from the downloaded source.
    ///
    /// When omitted, common font file extensions are included by default. If a path matches both
    /// `include` and `exclude`, `exclude` takes precedence.
    #[serde(
        default = "default_include",
        deserialize_with = "non_empty_vec::deserialize"
    )]
    pub(crate) include: Vec<PathPattern>,
    /// Glob patterns excluding files from installation even if they are matched by `include`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) exclude: Vec<PathPattern>,
}

impl From<PackageManifest> for PackageDefinition {
    fn from(manifest: PackageManifest) -> Self {
        let PackageManifest {
            name,
            display_name,
            version,
            description,
            aliases,
            faces,
            homepage,
            repository,
            license,
            sources,
        } = manifest;
        Self {
            id: PackageId::new(name, version),
            display_name,
            description,
            aliases,
            faces,
            homepage: homepage.map(|url| url.to_string()),
            repository: repository.map(|url| url.to_string()),
            license: license.map(|expr| expr.to_string()),
            sources: sources.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<PackageManifestSource> for PackageSource {
    fn from(source: PackageManifestSource) -> Self {
        let PackageManifestSource {
            url,
            hash,
            include,
            exclude,
        } = source;
        Self {
            url,
            hash,
            include,
            exclude,
        }
    }
}

impl PackageManifest {
    pub(crate) fn id(&self) -> PackageId {
        PackageId::new(self.name.clone(), self.version.clone())
    }
}

fn default_include() -> Vec<PathPattern> {
    vec![
        PathPattern::new("**/*.ttf").unwrap(),
        PathPattern::new("**/*.otf").unwrap(),
        PathPattern::new("**/*.ttc").unwrap(),
    ]
}

fn validate_nonempty_string_without_surrounding_whitespaces<E>(s: String) -> Result<String, E>
where
    E: serde::de::Error,
{
    let t = s.trim();
    if t.is_empty() || t != s {
        return Err(E::invalid_value(
            serde::de::Unexpected::Str(&s),
            &"a non-empty string without leading or trailing whitespace",
        ));
    }
    Ok(s)
}

mod option_nonempty_string_without_surrounding_whitespaces {
    use serde::Deserialize as _;

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Option::deserialize(deserializer)?
            .map(super::validate_nonempty_string_without_surrounding_whitespaces)
            .transpose()
    }
}

mod vec_nonempty_strings_without_surrounding_whitespaces {
    use serde::Deserialize as _;

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Vec::<String>::deserialize(deserializer)?
            .into_iter()
            .map(super::validate_nonempty_string_without_surrounding_whitespaces)
            .collect()
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

#[derive(Debug, Snafu)]
pub(crate) enum PackageManifestError {
    #[snafu(display("manifest file not found: {path}", path = path.display()))]
    NotFound { path: PathBuf },
    #[snafu(display(
        "failed to read manifest file: {path}", path = path.display()
    ))]
    ReadManifest { path: PathBuf, source: io::Error },
    #[snafu(display(
        "failed to deserialize manifest: {path}", path = path.display()
    ))]
    DeserializeManifest {
        path: PathBuf,
        #[snafu(source(from(toml::de::Error, Box::new)))]
        source: Box<toml::de::Error>,
    },
}

impl PackageManifest {
    pub(crate) fn read<P>(path: P) -> Result<Self, PackageManifestError>
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref();
        let manifest_str = fs::read_to_string(path).map_err(|source| {
            if source.kind() == io::ErrorKind::NotFound {
                NotFoundSnafu { path }.build()
            } else {
                ReadManifestSnafu { path }.into_error(source)
            }
        })?;
        let manifest = toml::from_str(&manifest_str).context(DeserializeManifestSnafu { path })?;
        Ok(manifest)
    }
}

#[cfg(test)]
mod tests {
    use std::{assert_matches, fs};

    use tempfile::TempDir;

    use super::*;
    use crate::util::testing;

    fn minimal_manifest_toml() -> &'static str {
        r#"
name = "example-font"
version = "0.1.0"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
include = ["*/*.ttf"]
"#
    }

    #[test]
    fn package_manifest_deserializes_minimal_manifest() {
        let manifest = testing::parse_manifest(minimal_manifest_toml());

        assert_eq!(manifest.name, "example-font");
        assert_eq!(manifest.display_name, None);
        assert_eq!(manifest.version, "0.1.0".parse().unwrap());
        assert_eq!(manifest.description, None);
        assert!(manifest.aliases.is_empty());
        assert!(manifest.faces.is_empty());
        assert_eq!(manifest.homepage, None);
        assert_eq!(manifest.repository, None);
        assert_eq!(manifest.license, None);
        assert_eq!(manifest.sources.len(), 1);
        assert_eq!(
            manifest.sources[0].url.as_str(),
            "https://example.com/example-font-0.1.0.zip"
        );
        assert_eq!(
            manifest.sources[0].hash.to_string(),
            "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        );
        assert_eq!(
            manifest.sources[0]
                .include
                .iter()
                .map(PathPattern::as_str)
                .collect::<Vec<_>>(),
            ["*/*.ttf"]
        );
        assert!(manifest.sources[0].exclude.is_empty());
    }

    #[test]
    fn package_manifest_deserializes_manifest_with_all_metadata_fields() {
        let manifest = testing::parse_manifest(
            r#"
name = "example-font"
display-name = "Example Font"
version = "0.1.0"
description = "Example font family for UI and coding"
aliases = ["Example Font UI"]
faces = ["Example Font Regular", "Example Font Bold", "Example Font UI Regular", "Example Font UI Bold"]
homepage = "https://example.com/example-font"
repository = "https://github.com/example/example-font"
license = "OFL-1.1"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
include = ["*/*.ttf"]
"#,
        );

        assert_eq!(manifest.name, "example-font");
        assert_eq!(manifest.display_name.as_deref(), Some("Example Font"));
        assert_eq!(manifest.version, "0.1.0".parse().unwrap());
        assert_eq!(
            manifest.description.as_deref(),
            Some("Example font family for UI and coding")
        );
        assert_eq!(manifest.aliases, ["Example Font UI"]);
        assert_eq!(
            manifest.faces,
            [
                "Example Font Regular",
                "Example Font Bold",
                "Example Font UI Regular",
                "Example Font UI Bold"
            ]
        );
        assert_eq!(
            manifest.homepage.as_ref().map(Url::as_str),
            Some("https://example.com/example-font")
        );
        assert_eq!(
            manifest.repository.as_ref().map(Url::as_str),
            Some("https://github.com/example/example-font")
        );
        assert_eq!(
            manifest.license.as_ref().map(ToString::to_string),
            Some("OFL-1.1".to_string())
        );
        assert_eq!(manifest.sources.len(), 1);
        assert_eq!(
            manifest.sources[0].url.as_str(),
            "https://example.com/example-font-0.1.0.zip"
        );
        assert_eq!(
            manifest.sources[0].hash.to_string(),
            "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        );
        assert_eq!(
            manifest.sources[0]
                .include
                .iter()
                .map(PathPattern::as_str)
                .collect::<Vec<_>>(),
            ["*/*.ttf"]
        );
    }

    #[test]
    fn package_manifest_rejects_invalid_license_expression() {
        let err = testing::try_parse_manifest(
            r#"
name = "example-font"
version = "0.1.0"
license = "not-a-valid-spdx"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
"#,
        )
        .unwrap_err();

        assert!(err.to_string().contains("invalid SPDX expression"));
    }

    #[test]
    fn package_manifest_rejects_empty_sources() {
        let err = testing::try_parse_manifest(
            r#"
sources = []

name = "example-font"
version = "0.1.0"
"#,
        )
        .unwrap_err();

        assert!(err.to_string().contains("a non-empty array"));
    }

    #[test]
    fn package_manifest_rejects_empty_include() {
        let err = testing::try_parse_manifest(
            r#"
name = "example-font"
version = "0.1.0"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
include = []
"#,
        )
        .unwrap_err();

        assert!(err.to_string().contains("a non-empty array"));
    }

    #[test]
    fn package_manifest_rejects_empty_description() {
        let err = testing::try_parse_manifest(
            r#"
name = "example-font"
version = "0.1.0"
description = ""

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
"#,
        )
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("a non-empty string without leading or trailing whitespace")
        );
    }

    #[test]
    fn package_manifest_rejects_empty_display_name() {
        let err = testing::try_parse_manifest(
            r#"
name = "example-font"
version = "0.1.0"
display-name = ""

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
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
        let err = testing::try_parse_manifest(
            r#"
name = "example-font"
version = "0.1.0"
description = " example-font "

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
"#,
        )
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("a non-empty string without leading or trailing whitespace")
        );
    }

    #[test]
    fn package_manifest_rejects_aliases_with_surrounding_whitespace() {
        let err = testing::try_parse_manifest(
            r#"
name = "example-font"
version = "0.1.0"
aliases = [" Example Font "]

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
"#,
        )
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("a non-empty string without leading or trailing whitespace")
        );
    }

    #[test]
    fn package_manifest_rejects_empty_faces() {
        let err = testing::try_parse_manifest(
            r#"
name = "example-font"
version = "0.1.0"
faces = [""]

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
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
        let err = testing::try_parse_manifest(
            r#"
name = "example-font"
version = "0.1.0"

[[sources]]
url = "file:///tmp/example-font_v0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
"#,
        )
        .unwrap_err();

        assert!(err.to_string().contains("a URL with http or https scheme"));
    }

    #[test]
    fn package_manifest_rejects_non_http_homepage_url() {
        let err = testing::try_parse_manifest(
            r#"
name = "example-font"
version = "0.1.0"
homepage = "file:///tmp/project"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
"#,
        )
        .unwrap_err();

        assert!(err.to_string().contains("a URL with http or https scheme"));
    }

    #[test]
    fn package_manifest_uses_default_include_when_omitted() {
        let manifest = testing::parse_manifest(
            r#"
name = "example-font"
version = "0.1.0"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
"#,
        );

        assert_eq!(
            manifest.sources[0]
                .include
                .iter()
                .map(PathPattern::as_str)
                .collect::<Vec<_>>(),
            ["**/*.ttf", "**/*.otf", "**/*.ttc"]
        );
        assert!(manifest.sources[0].exclude.is_empty());
    }

    #[test]
    fn package_manifest_deserializes_exclude_patterns() {
        let manifest = testing::parse_manifest(
            r#"
name = "example-font"
version = "0.1.0"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
include = ["fonts/*.ttf"]
exclude = ["fonts/exclude.ttf", "fonts/legacy/*.ttf"]
"#,
        );

        assert_eq!(
            manifest.sources[0]
                .exclude
                .iter()
                .map(PathPattern::as_str)
                .collect::<Vec<_>>(),
            ["fonts/exclude.ttf", "fonts/legacy/*.ttf"]
        );
    }

    #[test]
    fn package_manifest_read_reads_manifest_file() {
        let tempdir = TempDir::new().unwrap();
        let path = tempdir.path().join("manifest.toml");
        fs::write(&path, minimal_manifest_toml()).unwrap();

        let manifest = PackageManifest::read(&path).unwrap();

        assert_eq!(manifest.id().to_string(), "example-font@0.1.0");
    }

    #[test]
    fn package_manifest_read_returns_not_found_for_missing_file() {
        let tempdir = TempDir::new().unwrap();
        let path = tempdir.path().join("missing.toml");

        let err = PackageManifest::read(&path).unwrap_err();

        assert_matches!(err, PackageManifestError::NotFound { .. });
    }

    #[test]
    fn package_manifest_read_returns_read_error_for_directory() {
        let tempdir = TempDir::new().unwrap();

        let err = PackageManifest::read(tempdir.path()).unwrap_err();

        assert_matches!(err, PackageManifestError::ReadManifest { .. });
    }

    #[test]
    fn package_manifest_read_returns_deserialize_error_for_invalid_toml() {
        let tempdir = TempDir::new().unwrap();
        let path = tempdir.path().join("manifest.toml");
        fs::write(&path, "not valid toml").unwrap();

        let err = PackageManifest::read(&path).unwrap_err();

        assert_matches!(err, PackageManifestError::DeserializeManifest { .. });
    }
}
