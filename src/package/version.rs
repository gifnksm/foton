use std::{
    cmp::Ordering,
    fmt::{self, Display},
    str::FromStr,
    sync::{Arc, LazyLock},
};

use regex::bytes::Regex;
use serde::{Deserialize, Serialize};
use snafu::Snafu;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub(crate) struct PackageVersion(Arc<str>);

const VERSION_REGEX_STR: &str = r"(?x)
    ^
    [0-9]+
    (?: \.[0-9]+ )*
    (?:
        -[a-z]+
        (?: - (?: [a-z]+ | [0-9]+ ) )*
    )?
    $
";

#[derive(Debug, Snafu)]
pub(crate) enum ParsePackageVersionError {
    #[snafu(display(
        "invalid package version `{version}`: must use dot-separated numeric parts with an optional final suffix; see Package Manifest Reference for details"
    ))]
    InvalidFormat { version: String },
}

impl PackageVersion {
    pub(crate) fn new<V>(version: V) -> Result<Self, ParsePackageVersionError>
    where
        V: Into<String>,
    {
        static VERSION_REGEX: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(VERSION_REGEX_STR).unwrap());

        let version = version.into();
        snafu::ensure!(
            VERSION_REGEX.is_match(version.as_bytes()),
            InvalidFormatSnafu { version }
        );
        Ok(Self(version.into()))
    }
}

impl FromStr for PackageVersion {
    type Err = ParsePackageVersionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl TryFrom<String> for PackageVersion {
    type Error = ParsePackageVersionError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::from_str(&value)
    }
}

impl TryFrom<&str> for PackageVersion {
    type Error = ParsePackageVersionError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::from_str(value)
    }
}

impl Display for PackageVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl From<&PackageVersion> for PackageVersion {
    fn from(version: &PackageVersion) -> Self {
        version.clone()
    }
}

impl<'de> Deserialize<'de> for PackageVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::new(s).map_err(serde::de::Error::custom)
    }
}

impl PartialOrd for PackageVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PackageVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        let mut self_parts = self.0.split('.');
        let mut other_parts = other.0.split('.');
        loop {
            let self_part = self_parts.next();
            let other_part = other_parts.next();
            let (self_part, other_part) = match (self_part, other_part) {
                (None, None) => return Ordering::Equal,
                (None, Some(_)) => return Ordering::Less,
                (Some(_), None) => return Ordering::Greater,
                (Some(self_part), Some(other_part)) => (self_part, other_part),
            };
            let ord = compare_part(self_part, other_part);
            if ord.is_ne() {
                return ord;
            }
        }
    }
}

fn compare_part(a: &str, b: &str) -> Ordering {
    let (a_numeric, a_suffixes) = split_numeric_suffixes(a);
    let (b_numeric, b_suffixes) = split_numeric_suffixes(b);
    let ord = compare_numeric(a_numeric, b_numeric);
    if ord.is_ne() {
        return ord;
    }
    match (a_suffixes, b_suffixes) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(a_suffixes), Some(b_suffixes)) => compare_suffixes(a_suffixes, b_suffixes),
    }
}

fn split_numeric_suffixes(part: &str) -> (&str, Option<&str>) {
    part.split_once('-')
        .map_or((part, None), |(num, suffix)| (num, Some(suffix)))
}

fn compare_numeric(a: &str, b: &str) -> Ordering {
    let a_num = a.trim_start_matches('0');
    let b_num = b.trim_start_matches('0');
    // Compare numeric values without parsing to a fixed-width integer type.
    // All-zero strings become empty after trimming and are therefore treated as
    // numeric zero; if the numeric values are equal but the original strings
    // differ (e.g. "01" vs "1"), the one with fewer leading zeros is
    // considered smaller.
    (a_num.len().cmp(&b_num.len()))
        .then_with(|| a_num.cmp(b_num))
        .then_with(|| a.len().cmp(&b.len()))
}

fn compare_suffixes(a: &str, b: &str) -> Ordering {
    let mut a_suffixes = a.split('-');
    let mut b_suffixes = b.split('-');
    loop {
        let a_suffix = a_suffixes.next();
        let b_suffix = b_suffixes.next();
        let (a_suffix, b_suffix) = match (a_suffix, b_suffix) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(a_part), Some(b_part)) => (a_part, b_part),
        };
        let ord = compare_suffix(a_suffix, b_suffix);
        if ord.is_ne() {
            return ord;
        }
    }
}

#[expect(clippy::similar_names)]
fn compare_suffix(a: &str, b: &str) -> Ordering {
    let is_a_num = a.chars().all(|c| c.is_ascii_digit());
    let is_b_num = b.chars().all(|c| c.is_ascii_digit());
    match (is_a_num, is_b_num) {
        (true, true) => compare_numeric(a, b),
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (false, false) => a.cmp(b),
    }
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use super::PackageVersion;

    #[test]
    fn package_version_parses_expected_inputs() {
        let valid_cases = [
            "0",
            "00",
            "1",
            "01",
            "1.0",
            "1.0.0",
            "2024.05.11",
            "2407.24",
            "1.2.3",
            "1-rc",
            "1-rc-1",
            "1-alpha-beta",
            "1-alpha-01",
            "1.0-beta-2",
            "1.0.0-rc-1",
            "18446744073709551616",
            "1-rc-18446744073709551616",
        ];
        for input in valid_cases {
            let version: PackageVersion = input.parse().unwrap();
            assert_eq!(version.to_string(), input);
        }

        let invalid_cases = [
            "",
            ".1",
            "1.",
            "1..0",
            "v1",
            "1_0",
            "1-",
            "1--rc",
            "1-RC",
            "1-rc10",
            "1-beta2",
            "1-alpha_beta",
            "1-alpha-2beta",
            "1-rc.10",
            "1.0-beta.2",
            "1-alpha.0.1",
            "1-rc.0",
        ];
        for input in invalid_cases {
            assert!(input.parse::<PackageVersion>().is_err(), "{input}");
        }
    }

    #[test]
    fn package_version_reports_expected_syntax_for_invalid_input() {
        let err = "1-rc10".parse::<PackageVersion>().unwrap_err();
        assert_eq!(
            err.to_string(),
            "invalid package version `1-rc10`: must use dot-separated numeric parts with an optional final suffix; see Package Manifest Reference for details"
        );
    }

    #[test]
    fn package_version_orders_expected_inputs() {
        let ordered_groups = [
            &["0", "00"][..],
            &["1-rc", "1-rc-1", "1-rc-01", "1-rc-2", "1-rc-10"][..],
            &["1-rc-18446744073709551616", "1-rc-a"][..],
            &[
                "1",
                "1.0-rc",
                "1.0-rc-1",
                "1.0",
                "1.0.0-rc-1",
                "1.0.0",
                "1.00",
            ][..],
            &["1.2", "1.2.0", "1.10.0"][..],
            &["5", "05", "005"][..],
            &["2024.5.11", "2024.05.11"][..],
            &["1.0.0-alpha", "1.0.0-beta"][..],
            &["1.0.0-rc-2", "1.0.0-rc-10"][..],
            &["1.0.0-rc-1", "1.0.0-rc-a"][..],
            &["1.0.0-rc", "1.0.0"][..],
            &["2024.05.11-beta-2", "2024.05.11"][..],
            &["18446744073709551616", "18446744073709551617"][..],
        ];

        for group in ordered_groups {
            let parsed = group
                .iter()
                .map(|input| input.parse::<PackageVersion>().unwrap())
                .collect::<Vec<_>>();

            for (i, lhs) in parsed.iter().enumerate() {
                assert_eq!(lhs.cmp(lhs), Ordering::Equal);
                for (j, rhs) in parsed.iter().enumerate() {
                    let expected = i.cmp(&j);
                    assert_eq!(lhs.cmp(rhs), expected, "lhs={lhs}, rhs={rhs}");
                    assert_eq!(rhs.cmp(lhs), expected.reverse(), "lhs={lhs}, rhs={rhs}");
                    assert_eq!(lhs.partial_cmp(rhs), Some(expected), "lhs={lhs}, rhs={rhs}");
                    assert_eq!(
                        rhs.partial_cmp(lhs),
                        Some(expected.reverse()),
                        "lhs={lhs}, rhs={rhs}"
                    );
                }
            }
        }
    }
}
