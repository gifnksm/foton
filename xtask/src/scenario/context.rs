use std::{process::Command, sync::Arc};

use color_eyre::eyre::{self, ensure};

use crate::{
    report::{ExecResult, ReportContext},
    scenario::{
        ScenarioParameters,
        model::{ListFontEntry, ListPackageEntry},
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

    #[track_caller]
    pub(in crate::scenario) fn exec_foton<F>(&self, f: F) -> eyre::Result<Arc<ExecResult>>
    where
        F: FnOnce(&mut Command),
    {
        let mut cmd = Command::new(&self.params.foton_exe);
        cmd.args(["--no-confirm", "--exit-on-lock"]);
        f(&mut cmd);
        crate::util::process::exec_command(self.cx, "foton", &self.params.output_dir, &mut cmd)
    }

    #[track_caller]
    pub(in crate::scenario) fn exec_foton_with_args<I, S>(
        &self,
        args: I,
    ) -> eyre::Result<Arc<ExecResult>>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        self.exec_foton(|cmd| {
            cmd.args(args);
        })
    }

    #[track_caller]
    pub(in crate::scenario) fn query_all_managed_packages(
        &self,
    ) -> eyre::Result<Vec<ListPackageEntry>> {
        self.exec_foton_with_args(["list", "--format", "jsonl"])?
            .ensure_success()?
            .deserialize_stdout_as_jsonl()
    }

    #[track_caller]
    pub(in crate::scenario) fn query_managed_packages(
        &self,
        pkg_spec: &str,
    ) -> eyre::Result<Vec<ListPackageEntry>> {
        self.exec_foton_with_args(["list", "--format", "jsonl", pkg_spec])?
            .ensure_success()?
            .deserialize_stdout_as_jsonl()
    }

    #[track_caller]
    pub(in crate::scenario) fn query_all_package_active_fonts(
        &self,
    ) -> eyre::Result<Vec<ListFontEntry>> {
        self.exec_foton_with_args(["font", "list", "--format", "jsonl"])?
            .ensure_success()?
            .deserialize_stdout_as_jsonl()
    }

    #[track_caller]
    pub(in crate::scenario) fn query_package_active_fonts(
        &self,
        pkg_spec: &str,
    ) -> eyre::Result<Vec<ListFontEntry>> {
        let fonts = self
            .query_all_package_active_fonts()?
            .into_iter()
            .filter(|font| font.location.is_pkg_font(pkg_spec))
            .collect();
        Ok(fonts)
    }

    #[track_caller]
    pub(in crate::scenario) fn ensure_package_not_managed(
        &self,
        pkg_spec: &str,
    ) -> eyre::Result<()> {
        let managed_packages = self.query_all_managed_packages()?;
        ensure!(
            managed_packages
                .iter()
                .all(|pkg| !pkg.matches_spec(pkg_spec)),
            "package `{}` is still managed",
            pkg_spec,
        );
        Ok(())
    }

    #[track_caller]
    pub(in crate::scenario) fn ensure_package_installed(
        &self,
        pkg_spec: &str,
    ) -> eyre::Result<ListPackageEntry> {
        let mut managed_packages = self.query_managed_packages(pkg_spec)?;
        ensure!(
            managed_packages.len() == 1,
            "expected exactly one managed package matching `{}`, found {}",
            pkg_spec,
            managed_packages.len(),
        );
        let pkg = managed_packages.pop().unwrap();
        Ok(pkg)
    }

    #[track_caller]
    pub(in crate::scenario) fn ensure_package_has_no_active_fonts(
        &self,
        pkg_spec: &str,
    ) -> eyre::Result<()> {
        let active_fonts = self.query_all_package_active_fonts()?;
        ensure!(
            active_fonts
                .iter()
                .all(|font| !font.location.is_pkg_font(pkg_spec)),
            "package `{}` still has active fonts",
            pkg_spec,
        );
        Ok(())
    }

    #[track_caller]
    pub(in crate::scenario) fn ensure_package_has_active_fonts(
        &self,
        pkg_spec: &str,
    ) -> eyre::Result<Vec<ListFontEntry>> {
        let active_fonts = self.query_package_active_fonts(pkg_spec)?;
        ensure!(
            !active_fonts.is_empty(),
            "package `{}` has no active fonts",
            pkg_spec,
        );
        Ok(active_fonts)
    }
}
