use std::collections::BTreeMap;

use config::{Config, ConfigError};
use serde::{Deserialize, Serialize};

use crate::{
    registry::{RegistryId, RegistrySource},
    util::app_dirs::AppDirs,
};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FotonConfig {
    pub(crate) install: InstallConfig,
    #[serde(default)]
    pub(crate) registries: BTreeMap<RegistryId, Registry>,
}

#[derive(Debug, Deserialize, Serialize)]
#[expect(clippy::struct_field_names)]
#[serde(deny_unknown_fields)]
pub(crate) struct InstallConfig {
    pub(crate) max_archive_size_bytes: u64,
    pub(crate) max_extracted_files: usize,
    pub(crate) max_extracted_file_size_bytes: u64,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Registry {
    pub(crate) source: RegistrySource,
    #[serde(default = "default_true")]
    pub(crate) enabled: bool,
}

fn default_true() -> bool {
    true
}

impl Default for FotonConfig {
    fn default() -> Self {
        Self {
            install: InstallConfig::default(),
            registries: default_registries(),
        }
    }
}

fn default_registries() -> BTreeMap<RegistryId, Registry> {
    [(
        RegistryId::foton(),
        Registry {
            source: RegistrySource::foton(),
            enabled: true,
        },
    )]
    .into_iter()
    .collect()
}

impl Default for InstallConfig {
    fn default() -> Self {
        Self {
            max_archive_size_bytes: 100 * 1024 * 1024, // 100 MiB
            max_extracted_files: 1000,
            max_extracted_file_size_bytes: 50 * 1024 * 1024, // 50 MiB
        }
    }
}

pub(crate) fn load_config(app_dirs: &AppDirs) -> Result<FotonConfig, ConfigError> {
    let default_config = Config::try_from(&FotonConfig::default())?;
    let config = Config::builder()
        .add_source(default_config)
        .add_source(config::File::from(app_dirs.config_file().as_path()).required(false))
        .build()?;
    config.try_deserialize()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::util::testing;

    use super::*;

    #[test]
    fn load_config_returns_default_when_file_does_not_exist() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();

        let config = load_config(&app_dirs).unwrap();

        assert_eq!(
            config.install.max_archive_size_bytes,
            FotonConfig::default().install.max_archive_size_bytes
        );
        assert_eq!(
            config.install.max_extracted_files,
            FotonConfig::default().install.max_extracted_files
        );
        assert_eq!(
            config.install.max_extracted_file_size_bytes,
            FotonConfig::default().install.max_extracted_file_size_bytes
        );
        assert_eq!(config.registries, FotonConfig::default().registries);
    }

    #[test]
    fn load_config_overrides_defaults_from_config_file() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        fs::write(
            app_dirs.config_file(),
            r#"
[install]
max_archive_size_bytes = 123
max_extracted_files = 456
max_extracted_file_size_bytes = 789

[registries.foton]
source = "git+https://example.com/custom.git"
enabled = false

[registries.local]
source = "local+C:/registry"
"#,
        )
        .unwrap();

        let config = load_config(&app_dirs).unwrap();

        assert_eq!(config.install.max_archive_size_bytes, 123);
        assert_eq!(config.install.max_extracted_files, 456);
        assert_eq!(config.install.max_extracted_file_size_bytes, 789);
        assert_eq!(
            config.registries[&RegistryId::foton()].source,
            RegistrySource::try_from("git+https://example.com/custom.git").unwrap()
        );
        assert!(!config.registries[&RegistryId::foton()].enabled);
        assert_eq!(
            config.registries[&RegistryId::new("local").unwrap()].source,
            RegistrySource::try_from("local+C:/registry").unwrap()
        );
        assert!(config.registries[&RegistryId::new("local").unwrap()].enabled);
    }

    #[test]
    fn load_config_returns_error_for_invalid_config_file() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        fs::write(
            app_dirs.config_file(),
            r#"
[install]
max_archive_size_bytes = "invalid"
"#,
        )
        .unwrap();

        let err = load_config(&app_dirs).unwrap_err();
        let err = err.to_string();

        assert!(err.contains("max_archive_size_bytes"));
    }

    #[test]
    fn load_config_returns_error_for_unknown_root_key() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        fs::write(
            app_dirs.config_file(),
            r"
foton_unknown_key = 123
",
        )
        .unwrap();

        let err = load_config(&app_dirs).unwrap_err();
        let err = err.to_string();

        assert!(err.contains("foton_unknown_key"));
    }

    #[test]
    fn load_config_returns_error_for_unknown_install_key() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        fs::write(
            app_dirs.config_file(),
            r"
[install]
foton_unknown_key = 123
",
        )
        .unwrap();

        let err = load_config(&app_dirs).unwrap_err();
        let err = err.to_string();

        assert!(err.contains("foton_unknown_key"));
    }

    #[test]
    fn load_config_uses_default_enabled_for_registry() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        fs::write(
            app_dirs.config_file(),
            r#"
[registries.local]
source = "local+C:/registry"
"#,
        )
        .unwrap();

        let config = load_config(&app_dirs).unwrap();

        assert!(config.registries[&RegistryId::new("local").unwrap()].enabled);
    }

    #[test]
    fn load_config_returns_error_for_invalid_registry_source() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        fs::write(
            app_dirs.config_file(),
            r#"
[registries.local]
source = "https://example.com/registry.git"
"#,
        )
        .unwrap();

        let err = load_config(&app_dirs).unwrap_err();
        let err = err.to_string();

        assert!(err.contains("protocol is missing"));
    }

    #[test]
    fn load_config_returns_error_for_unknown_registry_key() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        fs::write(
            app_dirs.config_file(),
            r#"
[registries.foton]
source = "git+https://example.com/custom.git"
foton_unknown_key = true
"#,
        )
        .unwrap();

        let err = load_config(&app_dirs).unwrap_err();
        let err = err.to_string();

        assert!(err.contains("foton_unknown_key"));
    }
}
