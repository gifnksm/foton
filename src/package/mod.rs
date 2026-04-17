use std::fmt::{self, Display};

#[derive(Debug, Clone)]
pub(crate) struct PackageId {
    pub(crate) name: String,
    pub(crate) version: String,
}

impl Display for PackageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.name, self.version)
    }
}
