use std::{process::Command, sync::Arc};

use color_eyre::eyre;

use crate::{
    report::{ExecResult, ReportContext},
    scenario::{
        model::{ListFontEntry, ListPackageEntry},
        params::ScenarioParameters,
    },
};

#[derive(Debug)]
pub(in crate::scenario) struct ScenarioContext<'a> {
    cx: &'a ReportContext,
    params: &'a ScenarioParameters,
}

impl<'a> ScenarioContext<'a> {
    pub(in crate::scenario) fn new(cx: &'a ReportContext, params: &'a ScenarioParameters) -> Self {
        Self { cx, params }
    }

    pub(in crate::scenario) fn params(&self) -> &'a ScenarioParameters {
        self.params
    }

    pub(in crate::scenario) fn exec_foton<F>(&self, f: F) -> eyre::Result<Arc<ExecResult>>
    where
        F: FnOnce(&mut Command),
    {
        let mut cmd = Command::new(&self.params.foton_exe);
        f(&mut cmd);
        crate::util::process::exec_command(self.cx, "foton", &self.params.output_dir, &mut cmd)
    }

    pub(in crate::scenario) fn list_packages(&self) -> eyre::Result<Vec<ListPackageEntry>> {
        self.exec_foton(|cmd| {
            cmd.args(["list", "--exit-on-lock", "--format", "jsonl"]);
        })?
        .ensure_success()?
        .deserialize_stdout_as_jsonl()
    }

    pub(in crate::scenario) fn list_fonts(&self) -> eyre::Result<Vec<ListFontEntry>> {
        self.exec_foton(|cmd| {
            cmd.args(["font", "list", "--exit-on-lock", "--format", "jsonl"]);
        })?
        .ensure_success()?
        .deserialize_stdout_as_jsonl()
    }

    pub(in crate::scenario) fn list_package_fonts(&self) -> eyre::Result<Vec<ListFontEntry>> {
        let fonts = self
            .list_fonts()?
            .into_iter()
            .filter(|font| font.location.ty.is_package_dir())
            .collect();
        Ok(fonts)
    }
}
