pub(crate) use self::{
    install::*, manifest::*, registry::*, repair::*, target::*, uninstall::*, update::*,
};

mod install;
mod manifest;
mod registry;
mod repair;
mod target;
#[cfg(test)]
mod testing;
mod uninstall;
mod update;
