use std::{fmt::Display, str::FromStr};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum GenericDigest {
    Sha256(Sha256Digest),
}

impl GenericDigest {
    pub(crate) fn hasher(&self) -> GenericHasher {
        match self {
            Self::Sha256(_) => GenericHasher::Sha256(Sha256::new()),
        }
    }
}

impl Display for GenericDigest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sha256(digest) => write!(f, "sha256:{digest}"),
        }
    }
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum GenericDigestParseError {
    #[display("invalid sha256 digest")]
    Sha256 {
        #[error(source)]
        source: Sha256DigestParseError,
    },
    #[display("unsupported digest algorithm: `{algorithm}`")]
    NotSupported { algorithm: String },
    #[display("missing algorithm prefix in digest string")]
    NoAlgorithmPrefix,
}

impl FromStr for GenericDigest {
    type Err = GenericDigestParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let Some((algorithm, body)) = s.split_once(':') else {
            return Err(Self::Err::NoAlgorithmPrefix);
        };
        match algorithm {
            "sha256" => {
                let digest = Sha256Digest::from_str(body)
                    .map_err(|source| GenericDigestParseError::Sha256 { source })?;
                Ok(Self::Sha256(digest))
            }
            _ => Err(GenericDigestParseError::NotSupported {
                algorithm: algorithm.to_string(),
            }),
        }
    }
}

impl Serialize for GenericDigest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_string().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for GenericDigest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_str(&s).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, derive_more::From)]
pub(crate) enum GenericHasher {
    Sha256(Sha256),
}

impl GenericHasher {
    pub(crate) fn update(&mut self, data: &[u8]) {
        match self {
            Self::Sha256(hasher) => hasher.update(data),
        }
    }

    pub(crate) fn finalize(self) -> GenericDigest {
        match self {
            Self::Sha256(hasher) => GenericDigest::Sha256(Sha256Digest::new(hasher.finalize())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Sha256Digest([u8; 32]);

impl Display for Sha256Digest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for byte in &self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl Sha256Digest {
    pub(crate) fn new<B>(bytes: B) -> Self
    where
        B: Into<[u8; 32]>,
    {
        Self(bytes.into())
    }
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum Sha256DigestParseError {
    #[display("invalid length for sha256 digest: {length}")]
    InvalidLength { length: usize },
    #[display("invalid character in sha256 digest: `{ch:?}`")]
    InvalidCharacter { ch: char },
}

impl FromStr for Sha256Digest {
    type Err = Sha256DigestParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 64 {
            return Err(Self::Err::InvalidLength { length: s.len() });
        }

        let mut chars = s.chars();
        let mut bytes = [0; 32];
        for byte in &mut bytes {
            let (Some(c1), Some(c2)) = (chars.next(), chars.next()) else {
                return Err(Self::Err::InvalidLength { length: s.len() });
            };
            let Some(h1) = c1.to_digit(16) else {
                return Err(Self::Err::InvalidCharacter { ch: c1 });
            };
            let Some(h2) = c2.to_digit(16) else {
                return Err(Self::Err::InvalidCharacter { ch: c2 });
            };
            *byte = u8::try_from(h1 << 4 | h2).unwrap();
        }
        Ok(Self(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_digest_from_str_accepts_sha256_with_prefix() {
        let digest = GenericDigest::from_str(
            "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        )
        .unwrap();

        assert_eq!(
            digest.to_string(),
            "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        );
    }

    #[test]
    fn generic_digest_from_str_rejects_missing_prefix() {
        let err = GenericDigest::from_str(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        )
        .unwrap_err();

        assert!(matches!(err, GenericDigestParseError::NoAlgorithmPrefix));
    }

    #[test]
    fn generic_digest_from_str_rejects_unsupported_algorithm() {
        let err = GenericDigest::from_str(
            "sha512:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        )
        .unwrap_err();

        assert!(matches!(
            err,
            GenericDigestParseError::NotSupported { algorithm } if algorithm == "sha512"
        ));
    }

    #[test]
    fn sha256_digest_from_str_accepts_unprefixed_values() {
        let digest = Sha256Digest::from_str(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        )
        .unwrap();

        assert_eq!(
            digest.to_string(),
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        );
    }

    #[test]
    fn sha256_digest_from_str_rejects_invalid_inputs() {
        for input in [
            "",
            "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcde",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdez",
            "é0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcde",
        ] {
            let _ = Sha256Digest::from_str(input).unwrap_err();
        }
    }
}
