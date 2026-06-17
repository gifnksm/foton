pub(crate) use self::{
    activate::*, deactivate::*, install::*, manifest::*, registry::*, repair::*, target::*,
    uninstall::*, update::*,
};

mod activate;
mod deactivate;
mod install;
mod manifest;
mod registry;
mod repair;
mod target;
#[cfg(test)]
mod testing;
mod uninstall;
mod update;
