use std::{
    fs, io,
    path::{Path, PathBuf},
};

use reqwest::Url;
use serde::{Deserialize, Serialize};
use snafu::{IntoError as _, ResultExt as _, Snafu};

use crate::{
    package::{
        FontRule, IgnoreRule, PackageDefinition, PackageId, PackageName, PackageSource,
        PackageVersion,
    },
    util::{glob::PathGlob, hash::GenericDigest, path::FileName},
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
    pub(crate) contents: PackageSourceContents,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "kebab-case")]
#[serde(tag = "type")]
pub(crate) enum PackageSourceContents {
    FontFile(#[serde(default)] FontFileOptions),
    Archive(#[serde(default)] ArchiveOptions),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct FontFileOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) file_name: Option<FileName>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct ArchiveOptions {
    #[serde(
        default = "default_archive_fonts",
        deserialize_with = "non_empty_vec::deserialize",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub(crate) fonts: Vec<FontRule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) ignore: Vec<IgnoreRule>,
}

impl From<PackageManifest> for PackageDefinition {
    fn from(manifest: PackageManifest) -> Self {
        let PackageManifest {
            name,
            version,
            description,
            homepage,
            repository,
            license,
            sources,
        } = manifest;
        Self {
            id: PackageId::new(name, version),
            description,
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
            contents,
        } = source;
        Self {
            url,
            hash,
            contents,
        }
    }
}

impl PackageManifest {
    pub(crate) fn id(&self) -> PackageId {
        PackageId::new(self.name.clone(), self.version.clone())
    }
}

impl PackageSourceContents {
    #[cfg(test)]
    pub(crate) fn as_archive(&self) -> Option<&ArchiveOptions> {
        match self {
            Self::FontFile(_) => None,
            Self::Archive(archive_options) => Some(archive_options),
        }
    }
}

fn default_archive_fonts() -> Vec<FontRule> {
    vec![
        FontRule::glob(PathGlob::new("**/*.ttf").unwrap()),
        FontRule::glob(PathGlob::new("**/*.otf").unwrap()),
        FontRule::glob(PathGlob::new("**/*.ttc").unwrap()),
        FontRule::glob(PathGlob::new("**/*.otc").unwrap()),
    ]
}

pub(crate) fn validate_nonempty_string_without_surrounding_whitespaces<E>(
    s: String,
) -> Result<String, E>
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

pub(crate) mod option_nonempty_string_without_surrounding_whitespaces {
    use serde::Deserialize as _;

    pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Option::deserialize(deserializer)?
            .map(super::validate_nonempty_string_without_surrounding_whitespaces)
            .transpose()
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

    [sources.contents]
    type = "archive"
    fonts = [{ glob = "*/*.ttf" }]
    "#
    }

    #[test]
    fn package_manifest_deserializes_minimal_manifest() {
        let manifest = testing::parse_manifest(minimal_manifest_toml());

        assert_eq!(manifest.name, "example-font");
        assert_eq!(manifest.version, "0.1.0".parse().unwrap());
        assert_eq!(manifest.description, None);
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
        let archive = manifest.sources[0].contents.as_archive().unwrap();
        assert_eq!(
            archive
                .fonts
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["*/*.ttf"]
        );
        assert!(archive.ignore.is_empty());
    }

    #[test]
    fn package_manifest_deserializes_manifest_with_all_metadata_fields() {
        let manifest = testing::parse_manifest(
            r#"
name = "example-font"
version = "0.1.0"
description = "Example font family for UI and coding"
homepage = "https://example.com/example-font"
repository = "https://github.com/example/example-font"
license = "OFL-1.1"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

[sources.contents]
type = "archive"
fonts = [{ glob = "*/*.ttf" }]
"#,
        );

        assert_eq!(manifest.name, "example-font");
        assert_eq!(manifest.version, "0.1.0".parse().unwrap());
        assert_eq!(
            manifest.description.as_deref(),
            Some("Example font family for UI and coding")
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
        let archive = manifest.sources[0].contents.as_archive().unwrap();
        assert_eq!(
            archive
                .fonts
                .iter()
                .map(ToString::to_string)
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

[sources.contents]
type = "archive"
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
    fn package_manifest_rejects_empty_fonts() {
        let err = testing::try_parse_manifest(
            r#"
name = "example-font"
version = "0.1.0"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

[sources.contents]
type = "archive"
fonts = []
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
    fn package_manifest_rejects_description_with_surrounding_whitespace() {
        let err = testing::try_parse_manifest(
            r#"
name = "example-font"
version = "0.1.0"
description = " example-font "

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

[sources.contents]
type = "archive"
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

[sources.contents]
type = "archive"
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

[sources.contents]
type = "archive"
"#,
        )
        .unwrap_err();

        assert!(err.to_string().contains("a URL with http or https scheme"));
    }

    #[test]
    fn package_manifest_uses_default_files_when_omitted() {
        let manifest = testing::parse_manifest(
            r#"
name = "example-font"
version = "0.1.0"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

[sources.contents]
type = "archive"
"#,
        );

        let archive = manifest.sources[0].contents.as_archive().unwrap();
        assert_eq!(
            archive
                .fonts
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["**/*.ttf", "**/*.otf", "**/*.ttc", "**/*.otc"]
        );
        assert!(archive.ignore.is_empty());
    }

    #[test]
    fn package_manifest_deserializes_ignore_rules() {
        let manifest = testing::parse_manifest(
            r#"
name = "example-font"
version = "0.1.0"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

[sources.contents]
type = "archive"
fonts = [{ glob = "fonts/*.ttf" }]
ignore = ["fonts/exclude.ttf", { glob = "fonts/legacy/*.ttf" }]
"#,
        );

        let archive = manifest.sources[0].contents.as_archive().unwrap();
        assert_eq!(
            archive
                .ignore
                .iter()
                .map(ToString::to_string)
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
