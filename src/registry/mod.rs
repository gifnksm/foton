pub(crate) use self::{fetch::*, id::*, index::*, source::*};

mod fetch;
mod id;
mod index;
mod source;

#[derive(Debug)]
pub(crate) struct RegistrySpec {
    id: RegistryId,
    source: RegistrySource,
}

impl RegistrySpec {
    pub(crate) fn new(id: RegistryId, source: RegistrySource) -> Self {
        Self { id, source }
    }

    pub(crate) fn id(&self) -> &RegistryId {
        &self.id
    }
}
