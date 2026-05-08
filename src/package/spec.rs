use std::{fmt::Display, str::FromStr};

use snafu::{ResultExt as _, Snafu};

use crate::package::{
    PackageId, PackageName, PackageQualifiedName, ParsePackageIdError, ParsePackageNameError,
    ParsePackageQualifiedNameError,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, derive_more::From)]
pub(crate) enum PackageSpec {
    Name(PackageName),
    QualifiedName(PackageQualifiedName),
    Id(PackageId),
}

#[derive(Debug, Snafu)]
#[expect(clippy::enum_variant_names)]
pub(crate) enum ParsePackageSpecError {
    #[snafu(display("invalid qualified name in package specifier"))]
    InvalidQualifiedName {
        source: ParsePackageQualifiedNameError,
    },
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
        if s.contains('/') {
            let qualified_name = s.parse().context(InvalidQualifiedNameSnafu)?;
            return Ok(Self::QualifiedName(qualified_name));
        }
        let name = s.parse().context(InvalidNameSnafu)?;
        Ok(Self::Name(name))
    }
}

impl Display for PackageSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Name(name) => name.fmt(f),
            Self::QualifiedName(qualified_name) => qualified_name.fmt(f),
            Self::Id(id) => id.fmt(f),
        }
    }
}

impl From<&PackageSpec> for PackageSpec {
    fn from(spec: &PackageSpec) -> Self {
        spec.clone()
    }
}
