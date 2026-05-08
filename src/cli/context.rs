use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::{
    cli::{
        args::GlobalArgs,
        config::FotonConfig,
        reporter::{ReportScope, RootReporter, ScopedReporter},
    },
    util::app_dirs::AppDirs,
};

#[derive(Debug, Clone)]
pub(crate) struct Context<R> {
    app_id: Arc<str>,
    app_dirs: Arc<AppDirs>,
    global_args: Arc<GlobalArgs>,
    config: Arc<FotonConfig>,
    reporter: R,
    cancel_token: CancellationToken,
}

pub(crate) type RootContext = Context<RootReporter>;
pub(crate) type ReportContext<S> = Context<ScopedReporter<S>>;

impl<R> Context<R> {
    pub(crate) fn new(
        app_id: Arc<str>,
        app_dirs: Arc<AppDirs>,
        global_args: Arc<GlobalArgs>,
        config: Arc<FotonConfig>,
        reporter: R,
    ) -> Self {
        let cancel_token = CancellationToken::new();
        Self {
            app_id,
            app_dirs,
            global_args,
            config,
            reporter,
            cancel_token,
        }
    }

    pub(crate) fn app_id(&self) -> &str {
        &self.app_id
    }

    pub(crate) fn app_dirs(&self) -> &AppDirs {
        &self.app_dirs
    }

    pub(crate) fn reporter(&self) -> &R {
        &self.reporter
    }

    pub(crate) fn global_args(&self) -> &GlobalArgs {
        &self.global_args
    }

    pub(crate) fn config(&self) -> &FotonConfig {
        &self.config
    }

    pub(crate) fn cancel_token(&self) -> &CancellationToken {
        &self.cancel_token
    }
}

impl Context<RootReporter> {
    pub(crate) fn with_scope<S>(&self, scope: S) -> ReportContext<S>
    where
        S: ReportScope,
    {
        ReportContext {
            app_id: Arc::clone(&self.app_id),
            app_dirs: Arc::clone(&self.app_dirs),
            global_args: Arc::clone(&self.global_args),
            config: Arc::clone(&self.config),
            reporter: self.reporter.with_scope(scope),
            cancel_token: self.cancel_token.clone(),
        }
    }
}

impl<S> ReportContext<S>
where
    S: ReportScope,
{
    pub(crate) fn with_scope<T>(&self, scope: T) -> ReportContext<T>
    where
        T: ReportScope<Error = S::Error>,
    {
        ReportContext {
            app_id: Arc::clone(&self.app_id),
            app_dirs: Arc::clone(&self.app_dirs),
            global_args: Arc::clone(&self.global_args),
            config: Arc::clone(&self.config),
            reporter: self.reporter.with_scope(scope),
            cancel_token: self.cancel_token.clone(),
        }
    }
}
