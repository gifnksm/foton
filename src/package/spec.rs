use std::{fmt::Display, str::FromStr};

use snafu::{ResultExt as _, Snafu};

use crate::package::{PackageId, PackageName, ParsePackageIdError, ParsePackageNameError};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, derive_more::From)]
pub(crate) enum PackageSpec {
    Name(PackageName),
    Id(PackageId),
}

#[derive(Debug, Snafu)]
pub(crate) enum ParsePackageSpecError {
    #[snafu(display("invalid name in package specifier"))]
    InvalidName { source: ParsePackageNameError },
    #[snafu(display("invalid ID in package specifier"))]
    InvalidId { source: ParsePackageIdError },
}

impl FromStr for PackageSpec {
    type Err = ParsePackageSpecError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.contains('@') {
            let id = s.parse().context(InvalidIdSnafu)?;
            return Ok(Self::Id(id));
        }
        let name = s.parse().context(InvalidNameSnafu)?;
        Ok(Self::Name(name))
    }
}

impl Display for PackageSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Name(name) => name.fmt(f),
            Self::Id(id) => id.fmt(f),
        }
    }
}

impl From<&PackageSpec> for PackageSpec {
    fn from(spec: &PackageSpec) -> Self {
        spec.clone()
    }
}
