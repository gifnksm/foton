use crate::util::path::FileName;

pub(crate) use self::{
    dirs::*, id::*, manifest::*, name::*, namespace::*, qualified_name::*, spec::*, state::*,
    version::*,
};

mod dirs;
mod id;
mod manifest;
mod name;
mod namespace;
mod qualified_name;
mod spec;
mod state;
mod version;

#[derive(Debug, Clone)]
pub(crate) struct Package {
    id: PackageId,
    dirs: PackageDirs,
    entries: Vec<FontEntry>,
}

#[derive(Debug, Clone)]
pub(crate) struct FontEntry {
    title: String,
    file_name: FileName,
}

impl Package {
    pub(crate) fn new(id: PackageId, dirs: PackageDirs, entries: Vec<FontEntry>) -> Self {
        Self { id, dirs, entries }
    }

    pub(crate) fn id(&self) -> &PackageId {
        &self.id
    }

    pub(crate) fn dirs(&self) -> &PackageDirs {
        &self.dirs
    }

    pub(crate) fn entries(&self) -> &[FontEntry] {
        &self.entries
    }
}

impl FontEntry {
    pub(crate) fn new<T, F>(title: T, file_name: F) -> Self
    where
        T: Into<String>,
        F: Into<FileName>,
    {
        let title = title.into();
        let file_name = file_name.into();
        Self { title, file_name }
    }

    pub(crate) fn title(&self) -> &str {
        &self.title
    }

    pub(crate) fn file_name(&self) -> &FileName {
        &self.file_name
    }
}
