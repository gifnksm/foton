use std::{fmt::Display, str::FromStr};

use crate::package::{
    PackageId, PackageName, PackageQualifiedName, ParsePackageIdError, ParsePackageNameError,
    ParsePackageQualifiedNameError,
};

#[derive(Debug, Clone)]
pub(crate) enum PackageSpec {
    Name(PackageName),
    QualifiedName(PackageQualifiedName),
    Id(PackageId),
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
#[expect(clippy::enum_variant_names)]
pub(crate) enum ParsePackageSpecError {
    #[display("invalid qualified name in package specifier")]
    InvalidQualifiedName {
        #[error(source)]
        source: ParsePackageQualifiedNameError,
    },
    #[display("invalid name in package specifier")]
    InvalidName {
        #[error(source)]
        source: ParsePackageNameError,
    },
    #[display("invalid ID in package specifier")]
    InvalidId {
        #[error(source)]
        source: ParsePackageIdError,
    },
}

impl FromStr for PackageSpec {
    type Err = ParsePackageSpecError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.contains('@') {
            let id = s
                .parse()
                .map_err(|source| ParsePackageSpecError::InvalidId { source })?;
            return Ok(Self::Id(id));
        }
        if s.contains('/') {
            let qualified_name = s
                .parse()
                .map_err(|source| ParsePackageSpecError::InvalidQualifiedName { source })?;
            return Ok(Self::QualifiedName(qualified_name));
        }
        let name = s
            .parse()
            .map_err(|source| ParsePackageSpecError::InvalidName { source })?;
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
