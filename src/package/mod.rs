pub(crate) use self::{
    definition::*, dirs::*, id::*, manifest::*, name::*, rule::*, spec::*, state::*, version::*,
};
use crate::util::path::FileName;

mod definition;
mod dirs;
mod id;
mod manifest;
mod name;
mod rule;
mod spec;
mod state;
mod version;

#[derive(Debug, Clone)]
pub(crate) struct FontEntry {
    title: String,
    file_name: FileName,
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
