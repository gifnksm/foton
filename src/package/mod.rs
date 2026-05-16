use std::path::PathBuf;

pub(crate) use self::{dirs::*, id::*, manifest::*, name::*, spec::*, state::*, version::*};
use crate::util::path::FileName;

mod dirs;
mod id;
mod manifest;
mod name;
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
    source: FontSource,
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
    pub(crate) fn new<T, F>(title: T, file_name: F, source: FontSource) -> Self
    where
        T: Into<String>,
        F: Into<FileName>,
    {
        let title = title.into();
        let file_name = file_name.into();
        Self {
            title,
            file_name,
            source,
        }
    }

    pub(crate) fn title(&self) -> &str {
        &self.title
    }

    pub(crate) fn file_name(&self) -> &FileName {
        &self.file_name
    }

    pub(crate) fn source(&self) -> &FontSource {
        &self.source
    }
}

#[derive(Debug, Clone)]
pub(crate) struct FontSource {
    source_index: usize,
    path_in_source: PathBuf,
}

impl FontSource {
    pub(crate) fn new<P>(source_index: usize, path_in_source: P) -> Self
    where
        P: Into<PathBuf>,
    {
        let path_in_source = path_in_source.into();
        Self {
            source_index,
            path_in_source,
        }
    }

    pub(crate) fn source_index(&self) -> usize {
        self.source_index
    }

    pub(crate) fn path_in_source(&self) -> &PathBuf {
        &self.path_in_source
    }
}
