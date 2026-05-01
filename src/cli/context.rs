use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::{
    cli::{
        config::FotonConfig,
        reporter::{ReportScope, RootReporter, ScopedReporter},
    },
    util::app_dirs::AppDirs,
};

#[derive(Debug, Clone)]
pub(crate) struct Context<R> {
    app_id: Arc<str>,
    app_dirs: Arc<AppDirs>,
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
        config: Arc<FotonConfig>,
        reporter: R,
    ) -> Self {
        let cancel_token = CancellationToken::new();
        Self {
            app_id,
            app_dirs,
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
            config: Arc::clone(&self.config),
            reporter: self.reporter.with_scope(scope),
            cancel_token: self.cancel_token.clone(),
        }
    }

    pub(crate) fn scope(&self) -> &Arc<S> {
        self.reporter.scope()
    }
}
