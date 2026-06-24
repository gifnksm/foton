use std::{collections::BTreeMap, num::NonZero};

use config::{Config, ConfigError};
use serde::{Deserialize, Serialize};

use crate::{
    registry::{RegistryId, RegistrySource},
    util::app_dirs::AppDirs,
};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct FotonConfig {
    pub(crate) install: InstallConfig,
    #[serde(default)]
    pub(crate) registries: BTreeMap<RegistryId, RegistryConfig>,
}

#[derive(Debug, Deserialize, Serialize)]
#[expect(clippy::struct_field_names)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct InstallConfig {
    pub(crate) max_source_size_bytes: NonZero<u64>,
    pub(crate) max_fonts_per_package_source: NonZero<usize>,
    pub(crate) max_font_file_size_bytes: NonZero<u64>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct RegistryConfig {
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

fn default_registries() -> BTreeMap<RegistryId, RegistryConfig> {
    [(
        RegistryId::foton(),
        RegistryConfig {
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
            max_source_size_bytes: NonZero::new(512 * 1024 * 1024).unwrap(), // 512 MiB
            max_fonts_per_package_source: NonZero::new(1000).unwrap(),
            max_font_file_size_bytes: NonZero::new(128 * 1024 * 1024).unwrap(), // 128 MiB
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

    use super::*;
    use crate::util::testing;

    #[test]
    fn load_config_returns_default_when_file_does_not_exist() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();

        let config = load_config(&app_dirs).unwrap();

        assert_eq!(
            config.install.max_source_size_bytes,
            FotonConfig::default().install.max_source_size_bytes
        );
        assert_eq!(
            config.install.max_fonts_per_package_source,
            FotonConfig::default().install.max_fonts_per_package_source
        );
        assert_eq!(
            config.install.max_font_file_size_bytes,
            FotonConfig::default().install.max_font_file_size_bytes
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
max-source-size-bytes = 123
max-fonts-per-package-source = 456
max-font-file-size-bytes = 789

[registries.foton]
source = "git+https://example.com/custom.git"
enabled = false

[registries.local]
source = "local+C:/registry"
"#,
        )
        .unwrap();

        let config = load_config(&app_dirs).unwrap();

        assert_eq!(config.install.max_source_size_bytes.get(), 123);
        assert_eq!(config.install.max_fonts_per_package_source.get(), 456);
        assert_eq!(config.install.max_font_file_size_bytes.get(), 789);
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
max-source-size-bytes = "foton-invalid-value"
"#,
        )
        .unwrap();

        let err = load_config(&app_dirs).unwrap_err();
        let err = err.to_string();

        assert!(err.contains("max-source-size-bytes"));
        assert!(err.contains("foton-invalid-value"));
    }

    #[test]
    fn load_config_returns_error_for_unknown_root_key() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        fs::write(
            app_dirs.config_file(),
            r"
foton-unknown-key = 123
",
        )
        .unwrap();

        let err = load_config(&app_dirs).unwrap_err();
        let err = err.to_string();

        assert!(err.contains("foton-unknown-key"));
    }

    #[test]
    fn load_config_returns_error_for_unknown_install_key() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        fs::write(
            app_dirs.config_file(),
            r"
[install]
foton-unknown-key = 123
",
        )
        .unwrap();

        let err = load_config(&app_dirs).unwrap_err();
        let err = err.to_string();

        assert!(err.contains("foton-unknown-key"));
    }

    #[test]
    fn load_config_returns_error_for_zero_install_limit() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        fs::write(
            app_dirs.config_file(),
            r"
[install]
max-source-size-bytes = 0
",
        )
        .unwrap();

        let err = load_config(&app_dirs).unwrap_err();
        let err = err.to_string();

        assert!(err.contains("max-source-size-bytes"));
        assert!(err.contains('0'));
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
foton-unknown-key = true
"#,
        )
        .unwrap();

        let err = load_config(&app_dirs).unwrap_err();
        let err = err.to_string();

        assert!(err.contains("foton-unknown-key"));
    }
}
