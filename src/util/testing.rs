use std::{ops::Deref, sync::Arc};

use tempfile::TempDir;

use crate::{
    cli::{config::Config, context::RootContext},
    util::{app_dirs::AppDirs, path::AbsolutePath, reporter::RootReporter},
};

const APP_ID: &str = "io.github.gifnksm.foton-test";

#[derive(Debug)]
pub(crate) struct TempdirContext {
    _tempdir_guard: TempDir,
    cx: RootContext,
}

impl TempdirContext {
    pub(crate) fn new() -> Self {
        let tempdir = tempfile::tempdir().unwrap();
        let data_local_dir = AbsolutePath::new(tempdir.path()).unwrap();
        let app_dirs = AppDirs::new_for_test(data_local_dir);
        let config = Config::default();
        let reporter = RootReporter::message_reporter();
        let cx = RootContext::new(
            APP_ID.into(),
            Arc::new(app_dirs),
            Arc::new(config),
            reporter,
        );
        Self {
            _tempdir_guard: tempdir,
            cx,
        }
    }
}

impl Deref for TempdirContext {
    type Target = RootContext;

    fn deref(&self) -> &Self::Target {
        &self.cx
    }
}
