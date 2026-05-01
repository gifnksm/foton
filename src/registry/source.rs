use std::{
    fmt::{self, Display},
    str::FromStr,
    sync::LazyLock,
};

use serde::{Deserialize, Serialize};
use snafu::{OptionExt as _, ResultExt as _, Snafu};
use url::Url;

use crate::util::path::AbsolutePath;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum RegistrySource {
    Git(Url),
    Local(AbsolutePath),
}

static FOTON_REGISTRY_SOURCE: LazyLock<RegistrySource> = LazyLock::new(|| {
    RegistrySource::Git(
        "https://github.com/gifnksm/foton-registry.git"
            .parse()
            .unwrap(),
    )
});

impl RegistrySource {
    pub(crate) fn foton() -> Self {
        FOTON_REGISTRY_SOURCE.clone()
    }
}

#[derive(Debug, Snafu)]
pub(crate) enum RegistrySourceError {
    #[snafu(display("registry source protocol is missing"))]
    ProtocolMissing,
    #[snafu(display("unknown registry source protocol `{protocol}`"))]
    UnknownProtocol { protocol: String },
    #[snafu(display("invalid git URL: {url}"))]
    InvalidGitUrl {
        url: String,
        source: url::ParseError,
    },
    #[snafu(display("local path must be absolute path: {path}"))]
    RelativeLocalPath { path: String },
}

impl FromStr for RegistrySource {
    type Err = RegistrySourceError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let Some((protocol, path)) = s.split_once('+') else {
            return Err(ProtocolMissingSnafu.build());
        };
        match protocol {
            "git" => {
                let url = Url::parse(path).context(InvalidGitUrlSnafu { url: path })?;
                Ok(Self::Git(url))
            }
            "local" => {
                let path = AbsolutePath::new(path).context(RelativeLocalPathSnafu { path })?;
                Ok(Self::Local(path))
            }
            _ => Err(UnknownProtocolSnafu { protocol }.build()),
        }
    }
}

impl TryFrom<String> for RegistrySource {
    type Error = RegistrySourceError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::from_str(&value)
    }
}

impl TryFrom<&str> for RegistrySource {
    type Error = RegistrySourceError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::from_str(value)
    }
}

impl Display for RegistrySource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegistrySource::Git(url) => write!(f, "git+{url}"),
            RegistrySource::Local(path) => write!(f, "local+{}", path.display()),
        }
    }
}

impl Serialize for RegistrySource {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_string().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RegistrySource {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_str(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr as _;

    use serde_json::{from_str, to_string};

    use super::*;

    #[test]
    fn registry_source_from_str_accepts_valid_values() {
        let cases = [
            (
                "git+https://example.com/registry.git",
                RegistrySource::Git(Url::parse("https://example.com/registry.git").unwrap()),
            ),
            (
                "local+C:/registry",
                RegistrySource::Local(AbsolutePath::new("C:/registry").unwrap()),
            ),
        ];

        for (input, expected) in cases {
            assert_eq!(RegistrySource::from_str(input).unwrap(), expected);
        }
    }

    #[test]
    fn registry_source_from_str_rejects_invalid_values() {
        let cases = [
            (
                "https://example.com/registry.git",
                "registry source protocol is missing",
            ),
            (
                "unknown+https://example.com/registry.git",
                "unknown registry source protocol `unknown`",
            ),
            ("git+not a url", "invalid git URL: not a url"),
            (
                "local+registry",
                "local path must be absolute path: registry",
            ),
        ];

        for (input, expected) in cases {
            assert_eq!(
                RegistrySource::from_str(input).unwrap_err().to_string(),
                expected
            );
        }
    }

    #[test]
    fn registry_source_serde_roundtrip_preserves_value() {
        let source = RegistrySource::from_str("git+https://example.com/registry.git").unwrap();

        let serialized = to_string(&source).unwrap();
        let deserialized: RegistrySource = from_str(&serialized).unwrap();

        assert_eq!(serialized, "\"git+https://example.com/registry.git\"");
        assert_eq!(deserialized, source);
    }

    #[test]
    fn registry_source_deserialize_rejects_invalid_value() {
        let err = from_str::<RegistrySource>("\"local+registry\"").unwrap_err();

        assert!(err.to_string().contains("absolute path"));
    }
}
