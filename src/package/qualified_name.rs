use std::{
    fmt::{self, Display},
    str::FromStr,
};

use serde::{Deserialize, Serialize};
use snafu::{ResultExt as _, Snafu};

use crate::package::{
    PackageName, PackageNamespace, ParsePackageNameError, ParsePackageNamespaceError,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct PackageQualifiedName {
    namespace: PackageNamespace,
    name: PackageName,
}

impl PackageQualifiedName {
    pub(crate) fn new(namespace: PackageNamespace, name: PackageName) -> Self {
        Self { namespace, name }
    }

    pub(crate) fn namespace(&self) -> &PackageNamespace {
        &self.namespace
    }

    pub(crate) fn name(&self) -> &PackageName {
        &self.name
    }
}

impl Display for PackageQualifiedName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.namespace, self.name)
    }
}

impl From<&PackageQualifiedName> for PackageQualifiedName {
    fn from(pkg_name: &PackageQualifiedName) -> Self {
        pkg_name.clone()
    }
}

#[derive(Debug, Snafu)]
#[expect(clippy::enum_variant_names)]
pub(crate) enum ParsePackageQualifiedNameError {
    #[snafu(display("invalid package qualified name format"))]
    InvalidFormat,
    #[snafu(display("invalid namespace in package qualified name"))]
    InvalidNamespace { source: ParsePackageNamespaceError },
    #[snafu(display("invalid name in package qualified name"))]
    InvalidName { source: ParsePackageNameError },
}

impl FromStr for PackageQualifiedName {
    type Err = ParsePackageQualifiedNameError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let Some((namespace, name)) = s.split_once('/') else {
            return Err(InvalidFormatSnafu.build());
        };
        snafu::ensure!(
            !name.contains('/') && !namespace.is_empty() && !name.is_empty(),
            InvalidFormatSnafu
        );

        let namespace = namespace.parse().context(InvalidNamespaceSnafu)?;
        let name = name.parse().context(InvalidNameSnafu)?;

        Ok(Self { namespace, name })
    }
}

impl Serialize for PackageQualifiedName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_string().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PackageQualifiedName {
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
    use serde::de::value::{Error as ValueError, StrDeserializer};

    use super::*;

    #[test]
    fn package_qualified_name_parses_valid_string() {
        let qualified_name: PackageQualifiedName =
            "example-namespace/example-font".parse().unwrap();

        assert_eq!(qualified_name.namespace().to_string(), "example-namespace");
        assert_eq!(qualified_name.name().to_string(), "example-font");
        assert_eq!(qualified_name.to_string(), "example-namespace/example-font");
    }

    #[test]
    fn package_qualified_name_rejects_invalid_format() {
        for input in [
            "example-namespace",
            "example-font",
            "example-namespace/",
            "/example-font",
            "example-namespace/example/font",
        ] {
            assert!(matches!(
                input.parse::<PackageQualifiedName>(),
                Err(ParsePackageQualifiedNameError::InvalidFormat)
            ));
        }
    }

    #[test]
    fn package_qualified_name_reports_invalid_namespace() {
        let err = "0example-namespace/example-font"
            .parse::<PackageQualifiedName>()
            .unwrap_err();

        assert!(matches!(
            err,
            ParsePackageQualifiedNameError::InvalidNamespace { .. }
        ));
    }

    #[test]
    fn package_qualified_name_reports_invalid_name() {
        let err = "example-namespace/0example-font"
            .parse::<PackageQualifiedName>()
            .unwrap_err();

        assert!(matches!(
            err,
            ParsePackageQualifiedNameError::InvalidName { .. }
        ));
    }

    #[test]
    fn package_qualified_name_deserializes_from_string() {
        let deserializer = StrDeserializer::<ValueError>::new("example-namespace/example-font");
        let qualified_name = PackageQualifiedName::deserialize(deserializer).unwrap();

        assert_eq!(qualified_name.to_string(), "example-namespace/example-font");
    }
}
