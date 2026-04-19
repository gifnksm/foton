use std::{
    ffi::OsStr,
    fmt::{self, Display},
    path::Path,
    str::FromStr,
    sync::LazyLock,
};

use regex::Regex;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct PackageName(String);

const PACKAGE_NAME_REGEX_STR: &str = r"^[a-zA-Z][-_0-9a-zA-Z]*$";

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum PackageNameError {
    #[display(
        "invalid package name `{name}`: must start with an ASCII letter and contain only ASCII letters, digits, `-` or `_`"
    )]
    InvalidFormat { name: String },
}

impl PackageName {
    pub(crate) fn new<N>(name: N) -> Result<Self, PackageNameError>
    where
        N: Into<String>,
    {
        static NAME_REGEX: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(PACKAGE_NAME_REGEX_STR).unwrap());

        let name = name.into();
        if !NAME_REGEX.is_match(&name) {
            return Err(PackageNameError::InvalidFormat { name });
        }
        Ok(Self(name))
    }

    #[cfg(test)]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for PackageName {
    type Err = PackageNameError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl TryFrom<&str> for PackageName {
    type Error = PackageNameError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<String> for PackageName {
    type Error = PackageNameError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<&PackageName> for PackageName {
    fn from(name: &PackageName) -> Self {
        name.clone()
    }
}

impl Display for PackageName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl AsRef<str> for PackageName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl AsRef<OsStr> for PackageName {
    fn as_ref(&self) -> &OsStr {
        self.0.as_ref()
    }
}

impl AsRef<Path> for PackageName {
    fn as_ref(&self) -> &Path {
        self.0.as_ref()
    }
}

macro_rules! impl_partial_eq_for_package_name {
    ($($ty:ty),* $(,)?) => {
        $(
            impl PartialEq<$ty> for PackageName {
                fn eq(&self, other: &$ty) -> bool {
                    self.0 == *other
                }
            }

            impl PartialEq<PackageName> for $ty {
                fn eq(&self, other: &PackageName) -> bool {
                    *self == other.0
                }
            }
        )*
    };
}

impl_partial_eq_for_package_name!(String, str, &str);

impl PartialEq<&PackageName> for PackageName {
    fn eq(&self, other: &&PackageName) -> bool {
        self.0 == other.0
    }
}

impl PartialEq<PackageName> for &PackageName {
    fn eq(&self, other: &PackageName) -> bool {
        self.0 == other.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_name_new_accepts_valid_names() {
        for name_str in [
            "hackgen",
            "HackGen",
            "hackgen-nerd",
            "hackgen_nerd",
            "a0",
            "x",
        ] {
            let name = PackageName::new(name_str).unwrap();
            assert_eq!(name, name_str);
        }
    }

    #[test]
    fn package_name_new_rejects_invalid_names() {
        for name in [
            "",
            "0hackgen",
            "-hackgen",
            "_hackgen",
            "hackgen/nerd",
            r"hackgen\nerd",
            "hackgen:nerd",
        ] {
            PackageName::new(name).unwrap_err();
        }
    }
}
