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
    file_name: FileName,
}

impl FontEntry {
    pub(crate) fn new<F>(file_name: F) -> Self
    where
        F: Into<FileName>,
    {
        let file_name = file_name.into();
        Self { file_name }
    }

    pub(crate) fn file_name(&self) -> &FileName {
        &self.file_name
    }
}
