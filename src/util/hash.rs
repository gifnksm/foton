use std::{fmt::Display, str::FromStr};

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
        let s = s.strip_prefix("sha256:").unwrap_or(s);

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
    fn sha256_digest_from_str_accepts_prefixed_and_unprefixed_values() {
        let expected = Sha256Digest::from_str(
            "ed182e2a4b95792d94dea7932f6b45280b5ae353651be249d5f6b7867b788db7",
        )
        .unwrap();

        let actual = Sha256Digest::from_str(
            "sha256:ed182e2a4b95792d94dea7932f6b45280b5ae353651be249d5f6b7867b788db7",
        )
        .unwrap();

        assert_eq!(actual, expected);
        assert_eq!(
            actual.to_string(),
            "ed182e2a4b95792d94dea7932f6b45280b5ae353651be249d5f6b7867b788db7"
        );
    }

    #[test]
    fn sha256_digest_from_str_rejects_invalid_inputs() {
        for input in [
            "",
            "sha256:sha256:ed182e2a4b95792d94dea7932f6b45280b5ae353651be249d5f6b7867b788db7",
            "ed182e2a4b95792d94dea7932f6b45280b5ae353651be249d5f6b7867b788db",
            "ed182e2a4b95792d94dea7932f6b45280b5ae353651be249d5f6b7867b788dbz",
            "éd182e2a4b95792d94dea7932f6b45280b5ae353651be249d5f6b7867b788db7",
        ] {
            let _ = Sha256Digest::from_str(input).unwrap_err();
        }
    }
}
