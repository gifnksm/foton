use config::{Config, ConfigError};
use serde::{Deserialize, Serialize};

use crate::util::app_dirs::AppDirs;

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FotonConfig {
    pub(crate) install: InstallConfig,
}

#[derive(Debug, Deserialize, Serialize)]
#[expect(clippy::struct_field_names)]
#[serde(deny_unknown_fields)]
pub(crate) struct InstallConfig {
    pub(crate) max_archive_size_bytes: u64,
    pub(crate) max_extracted_files: usize,
    pub(crate) max_extracted_file_size_bytes: u64,
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
    }

    #[test]
    fn load_config_overrides_defaults_from_config_file() {
        let (_tempdir, app_dirs) = testing::make_app_dirs();
        fs::write(
            app_dirs.config_file(),
            r"
[install]
max_archive_size_bytes = 123
max_extracted_files = 456
max_extracted_file_size_bytes = 789
",
        )
        .unwrap();

        let config = load_config(&app_dirs).unwrap();

        assert_eq!(config.install.max_archive_size_bytes, 123);
        assert_eq!(config.install.max_extracted_files, 456);
        assert_eq!(config.install.max_extracted_file_size_bytes, 789);
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
}
