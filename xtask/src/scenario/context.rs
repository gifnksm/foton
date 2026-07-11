use std::{process::Command, sync::Arc};

use color_eyre::eyre::{self, ensure};

use crate::{
    report::{ExecResult, ReportContext},
    scenario::{
        ScenarioParameters,
        model::{ListFontEntry, ListPackageEntry},
    },
    util::{env as env_util, fs as fs_util},
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

    pub(in crate::scenario) fn install_fixture_config(&self, name: &str) -> eyre::Result<()> {
        let src_path = self
            .params
            .fixture_dir
            .join("config")
            .join(format!("{name}.toml"));
        let content = fs_util::read_to_string(format_args!("config {name}"), &src_path)?.replace(
            "__FIXTURE_DIR__",
            &self.params.fixture_dir.as_str().replace('\\', "/"),
        );
        fs_util::create_dir_all("foton config directory", env_util::foton_config_dir()?)?;
        let dst_path = env_util::foton_config_path()?;
        fs_util::write(format_args!("config {name}"), &dst_path, &content)?;
        Ok(())
    }

    #[track_caller]
    pub(in crate::scenario) fn exec_foton<F>(&self, f: F) -> eyre::Result<Arc<ExecResult>>
    where
        F: FnOnce(&mut Command),
    {
        let mut cmd = Command::new(&self.params.foton_exe);
        cmd.args(["--no-confirm", "--exit-on-lock"]);
        f(&mut cmd);
        self.cx.exec_command(&mut cmd)
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
    pub(in crate::scenario) fn ensure_package_managed(
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
        ensure!(
            pkg.matches_spec(pkg_spec),
            "expected managed package matching `{}`, found `{}`",
            pkg_spec,
            pkg.pkg_id
        );
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

    #[track_caller]
    pub(in crate::scenario) fn install_package(&self, pkg_spec: &str) -> eyre::Result<()> {
        self.exec_foton_with_args(["install", pkg_spec])?
            .ensure_success()?;
        self.ensure_package_managed(pkg_spec)?
            .ensure_installation_state(|state| state.is_installed())?
            .ensure_activation_state(|state| state.is_active())?;
        self.ensure_package_has_active_fonts(pkg_spec)?;
        Ok(())
    }

    #[track_caller]
    pub(in crate::scenario) fn install_package_no_activation(
        &self,
        pkg_spec: &str,
    ) -> eyre::Result<()> {
        self.exec_foton_with_args(["install", "--no-activate", pkg_spec])?
            .ensure_success()?;
        self.ensure_package_managed(pkg_spec)?
            .ensure_installation_state(|state| state.is_installed())?
            .ensure_activation_state(|state| state.is_inactive())?;
        self.ensure_package_has_no_active_fonts(pkg_spec)?;
        Ok(())
    }

    #[track_caller]
    pub(in crate::scenario) fn uninstall_package(&self, pkg_spec: &str) -> eyre::Result<()> {
        self.exec_foton_with_args(["uninstall", pkg_spec])?
            .ensure_success()?;
        self.ensure_package_not_managed(pkg_spec)?;
        self.ensure_package_has_no_active_fonts(pkg_spec)?;
        Ok(())
    }
}
