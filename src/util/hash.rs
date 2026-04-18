use std::{fmt::Display, str::FromStr};

use color_eyre::eyre::{self, WrapErr as _, eyre};
use sha2::{Digest as _, Sha256};

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

impl FromStr for Sha256Digest {
    type Err = eyre::Report;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.strip_prefix("sha256:").unwrap_or(s);

        if s.len() != 64 {
            return Err(eyre!("invalid length for sha256 digest: {}", s.len()));
        }

        let mut bytes = [0; 32];
        for (i, byte) in bytes.iter_mut().enumerate() {
            let hex = s
                .get(i * 2..i * 2 + 2)
                .ok_or_else(|| eyre!("invalid hex byte in sha256 digest: {s}"))?;
            *byte = u8::from_str_radix(hex, 16)
                .wrap_err_with(|| format!("invalid hex byte in sha256 digest: {hex}"))?;
        }
        Ok(Self(bytes))
    }
}

pub(crate) fn digest_from_bytes(bytes: &[u8]) -> Sha256Digest {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    Sha256Digest(digest.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_digest_from_str_accepts_prefixed_and_unprefixed_values() {
        let expected = Sha256Digest::from_str(
            "ed182e2a4b95792d94dea7932f6b45280b5ae353651be249d5f6b7867b788db7",
        )
        .expect("unprefixed digest should parse");

        let actual = Sha256Digest::from_str(
            "sha256:ed182e2a4b95792d94dea7932f6b45280b5ae353651be249d5f6b7867b788db7",
        )
        .expect("prefixed digest should parse");

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
            let _ = Sha256Digest::from_str(input).expect_err("invalid digest should be rejected");
        }
    }

    #[test]
    fn digest_from_bytes_matches_known_sha256() {
        let digest = digest_from_bytes(b"abc");

        assert_eq!(
            digest.to_string(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
