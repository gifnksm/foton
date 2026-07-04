use std::borrow::Cow;

use snafu::{IntoError as _, OptionExt as _, ResultExt as _, Snafu};
use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows_core::HSTRING;
use windows_registry::{CURRENT_USER, Key, Value};

use crate::{package::PackageId, util::path::AbsolutePath};

cfg_select! {
    all(test, not(build_for_sandbox)) => {
        // Registry-touching tests must run under the sandbox harness so they cannot
        // affect the user's real registry or session state.
        fn assert_sandbox_test_only() {
            if std::env::var_os("FOTON_UNSAFE_ALLOW_REGISTRY_TESTS").is_some() {
                return;
            }
            panic!("registry primitives must only run in sandbox tests; use `cargo xtask sandbox run --test` instead.");
        }
    }
    _ => {
        fn assert_sandbox_test_only() {}
    }
}

const USER_FONTS_REGISTRY_KEY_PATH: &str = r"Software\Microsoft\Windows NT\CurrentVersion\Fonts";

fn err_is_not_found(err: &windows_result::Error) -> bool {
    err.code() == ERROR_FILE_NOT_FOUND.to_hresult()
}

#[derive(Debug, Snafu)]
pub(crate) enum RegistryError {
    #[snafu(display("failed to create registry key: {key_path}"))]
    CreateRegistryKey {
        key_path: String,
        source: windows_core::Error,
    },
    #[snafu(display("failed to open registry key for read: {key_path}"))]
    OpenRegistryKeyForRead {
        key_path: String,
        source: windows_core::Error,
    },
    #[snafu(display("failed to open registry key for write: {key_path}"))]
    OpenRegistryKeyForWrite {
        key_path: String,
        source: windows_core::Error,
    },
    #[snafu(display("failed to enumerate values of registry key: {key_path}"))]
    EnumerateValues {
        key_path: String,
        source: windows_core::Error,
    },
    #[snafu(display("package `{pkg_id}` fonts already exist in registry key: {key_path}"))]
    PackageRegistryValuesAlreadyExist { pkg_id: PackageId, key_path: String },
    #[snafu(display("failed to set registry value `{value_name}` of registry key: {key_path}"))]
    SetValue {
        key_path: String,
        value_name: String,
        source: windows_core::Error,
    },
    #[snafu(display("failed to remove registry value `{value_name}` of registry key: {key_path}"))]
    RemoveValue {
        key_path: String,
        value_name: String,
        source: windows_core::Error,
    },
}

fn assert_value_name_prefix_segment(kind: &str, segment: &str) {
    assert!(
        !segment.contains(['\\', '\0', '@']) && !segment.chars().any(char::is_control),
        "invalid {kind} for registry value name segment: {segment:?}"
    );
}

fn assert_value_name_file_name(file_name: &str) {
    assert!(
        !file_name.contains(['\\', '\0']) && !file_name.chars().any(char::is_control),
        "invalid file name for registry value: {file_name:?}"
    );
}

fn reg_value_name(app_id: &str, pkg_id: &PackageId, file_name: &str) -> String {
    let name = pkg_id.name().as_str();
    let version = pkg_id.version().as_str();
    assert_value_name_prefix_segment("app id", app_id);
    assert_value_name_prefix_segment("package name", name);
    assert_value_name_prefix_segment("package version", version);
    assert_value_name_file_name(file_name);

    format!(r"{app_id}@{name}@{version}@{file_name}")
}

fn strip_value_name_prefix<'a>(
    app_id: &str,
    pkg_id: &PackageId,
    value_name: &'a str,
) -> Option<&'a str> {
    let name = pkg_id.name().as_str();
    let version = pkg_id.version().as_str();
    assert_value_name_prefix_segment("app id", app_id);
    assert_value_name_prefix_segment("package name", name);
    assert_value_name_prefix_segment("package version", version);

    value_name
        .strip_prefix(app_id)?
        .strip_prefix('@')?
        .strip_prefix(name)?
        .strip_prefix('@')?
        .strip_prefix(version)?
        .strip_prefix('@')
}

fn create_reg_key<P>(key_path: P) -> Result<Key, RegistryError>
where
    P: AsRef<str>,
{
    let key_path = key_path.as_ref();
    let key = match CURRENT_USER.create(key_path) {
        Ok(key) => key,
        Err(source) => {
            return Err(CreateRegistryKeySnafu { key_path }.into_error(source));
        }
    };
    Ok(key)
}

fn open_reg_key_for_read<P>(key_path: P) -> Result<Option<Key>, RegistryError>
where
    P: AsRef<str>,
{
    let key_path = key_path.as_ref();
    let key = match CURRENT_USER.open(key_path) {
        Ok(key) => key,
        Err(err) if err_is_not_found(&err) => {
            return Ok(None);
        }
        Err(source) => {
            return Err(OpenRegistryKeyForReadSnafu { key_path }.into_error(source));
        }
    };
    Ok(Some(key))
}

fn open_reg_key_for_write<P>(key_path: P) -> Result<Option<Key>, RegistryError>
where
    P: AsRef<str>,
{
    let key_path = key_path.as_ref();
    let key = match CURRENT_USER.options().read().write().open(key_path) {
        Ok(key) => key,
        Err(err) if err_is_not_found(&err) => {
            return Ok(None);
        }
        Err(source) => {
            return Err(OpenRegistryKeyForWriteSnafu { key_path }.into_error(source));
        }
    };
    Ok(Some(key))
}

#[derive(Debug)]
struct RegistryKey<'a> {
    key_path: Cow<'static, str>,
    app_id: &'a str,
}

impl<'a> RegistryKey<'a> {
    fn new(app_id: &'a str) -> Self {
        Self::with_key_path(app_id, Cow::Borrowed(USER_FONTS_REGISTRY_KEY_PATH))
    }

    fn with_key_path(app_id: &'a str, key_path: Cow<'static, str>) -> Self {
        assert_sandbox_test_only();
        Self { key_path, app_id }
    }

    fn list_valid_package_fonts(
        &self,
        pkg_id: &PackageId,
    ) -> Result<Vec<RegisteredFont>, RegistryError> {
        let key_path = self.key_path.as_ref();
        let Some(key) = open_reg_key_for_read(key_path)? else {
            return Ok(vec![]);
        };

        let values = key
            .values()
            .context(EnumerateValuesSnafu { key_path })?
            .filter_map(|(value_name, value)| {
                let file_name = strip_value_name_prefix(self.app_id, pkg_id, &value_name)?;
                RegisteredFont::from_reg_value(file_name, value)
            })
            .collect();
        Ok(values)
    }

    fn register_package_fonts<I, F>(
        &self,
        pkg_id: &PackageId,
        fonts: I,
    ) -> Result<(), RegistryError>
    where
        I: IntoIterator<Item = F>,
        F: AsRef<RegisteredFont>,
    {
        let key_path = self.key_path.as_ref();
        let key = create_reg_key(key_path)?;

        let mut registered_value_names = key
            .values()
            .with_context(|_| EnumerateValuesSnafu { key_path })?
            .filter(|(value_name, _value)| {
                strip_value_name_prefix(self.app_id, pkg_id, value_name).is_some()
            });
        if registered_value_names.next().is_some() {
            return Err(PackageRegistryValuesAlreadyExistSnafu { pkg_id, key_path }.build());
        }

        for font in fonts {
            let font = font.as_ref();
            let value_name = font.reg_value_name(self.app_id, pkg_id);
            let value = font.reg_value();
            key.set_value(&value_name, &value).context(SetValueSnafu {
                key_path,
                value_name,
            })?;
        }

        Ok(())
    }

    fn unregister_package_fonts(&self, pkg_id: &PackageId) -> Result<(), RegistryError> {
        let key_path = self.key_path.as_ref();
        let Some(key) = open_reg_key_for_write(key_path)? else {
            return Ok(());
        };

        let registered_value_names = key
            .values()
            .with_context(|_| EnumerateValuesSnafu { key_path })?
            .filter(|(value_name, _value)| {
                strip_value_name_prefix(self.app_id, pkg_id, value_name).is_some()
            })
            .map(|(value_name, _value)| value_name)
            .collect::<Vec<_>>();

        for value_name in registered_value_names {
            if let Err(source) = key.remove_value(&value_name)
                && !err_is_not_found(&source)
            {
                return Err(RemoveValueSnafu {
                    key_path,
                    value_name,
                }
                .into_error(source));
            }
        }

        Ok(())
    }
}

pub(crate) fn list_registered_valid_package_fonts(
    app_id: &str,
    pkg_id: &PackageId,
) -> Result<Vec<RegisteredFont>, RegistryError> {
    RegistryKey::new(app_id).list_valid_package_fonts(pkg_id)
}

pub(crate) fn register_package_fonts<I, F>(
    app_id: &str,
    pkg_id: &PackageId,
    fonts: I,
) -> Result<(), RegistryError>
where
    I: IntoIterator<Item = F>,
    F: AsRef<RegisteredFont>,
{
    RegistryKey::new(app_id).register_package_fonts(pkg_id, fonts)
}

pub(crate) fn unregister_package_fonts(
    app_id: &str,
    pkg_id: &PackageId,
) -> Result<(), RegistryError> {
    RegistryKey::new(app_id).unregister_package_fonts(pkg_id)
}

#[derive(Debug, Snafu)]
pub(crate) enum RegisteredFontError {
    #[snafu(display("font path has no file name: {path}", path = path.display()))]
    MissingFileName { path: AbsolutePath },
    #[snafu(display("font file name is not valid UTF-8: {path}", path = path.display()))]
    FileNameNotUtf8 { path: AbsolutePath },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RegisteredFont {
    file_name: String,
    path: AbsolutePath,
}

impl AsRef<RegisteredFont> for RegisteredFont {
    fn as_ref(&self) -> &RegisteredFont {
        self
    }
}

impl RegisteredFont {
    pub(crate) fn new<P>(path: P) -> Result<Self, RegisteredFontError>
    where
        P: Into<AbsolutePath>,
    {
        let path = path.into();
        let file_name = path
            .as_path()
            .file_name()
            .context(MissingFileNameSnafu { path: &path })?
            .to_str()
            .context(FileNameNotUtf8Snafu { path: &path })?
            .to_owned();
        Ok(Self { file_name, path })
    }

    pub(crate) fn path(&self) -> &AbsolutePath {
        &self.path
    }

    fn from_reg_value(file_name: &str, value: Value) -> Option<Self> {
        let path = HSTRING::try_from(value).ok()?.to_os_string();
        let path = AbsolutePath::new(&path)?;
        Some(Self {
            file_name: file_name.to_owned(),
            path,
        })
    }

    fn reg_value_name(&self, app_id: &str, pkg_id: &PackageId) -> String {
        reg_value_name(app_id, pkg_id, &self.file_name)
    }

    fn reg_value(&self) -> Value {
        Value::from(&HSTRING::from(self.path.as_path()))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fmt::Debug,
        process,
        str::FromStr as _,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use crate::APP_ID;

    use super::*;

    struct RegistryTestGuard<'a> {
        reg_key: RegistryKey<'a>,
    }

    impl RegistryTestGuard<'static> {
        fn new() -> Self {
            static TEST_ID: AtomicUsize = AtomicUsize::new(0);
            let test_id = TEST_ID.fetch_add(1, Ordering::Relaxed);
            let pid = process::id();
            let base = format!(r"{USER_FONTS_REGISTRY_KEY_PATH}\foton-test-{pid}-{test_id}");
            let app_id = APP_ID;
            let reg_key = RegistryKey::with_key_path(app_id, Cow::Owned(base));
            cleanup_registry(&reg_key);
            Self { reg_key }
        }
    }

    impl Drop for RegistryTestGuard<'_> {
        fn drop(&mut self) {
            cleanup_registry(&self.reg_key);
        }
    }

    fn with_registry_test<T, F>(f: F) -> T
    where
        F: FnOnce(&RegistryKey<'_>) -> T,
    {
        let guard = RegistryTestGuard::new();
        f(&guard.reg_key)
    }

    fn test_package_id(name: &str) -> PackageId {
        format!("registry-test-{name}@0.1.0").parse().unwrap()
    }

    fn registered_font<P>(path: P) -> RegisteredFont
    where
        P: TryInto<AbsolutePath>,
        P::Error: Debug,
    {
        let path = path.try_into().unwrap();
        RegisteredFont::new(path).unwrap()
    }

    fn cleanup_registry(reg_key: &RegistryKey<'_>) {
        let base = &reg_key.key_path;
        assert_ne!(base, USER_FONTS_REGISTRY_KEY_PATH);
        if let Err(err) = CURRENT_USER.remove_tree(base)
            && !err_is_not_found(&err)
        {
            panic!("failed to remove registry key `{base}`: {err:?}");
        }
    }

    fn list_value_names(reg_key: &RegistryKey<'_>) -> Vec<String> {
        let key_path = reg_key.key_path.as_ref();
        let key = match CURRENT_USER.open(key_path) {
            Ok(key) => key,
            Err(err) if err_is_not_found(&err) => return vec![],
            Err(err) => panic!("failed to open registry key `{key_path}`: {err:?}"),
        };

        key.values().unwrap().map(|(name, _)| name).collect()
    }

    #[test]
    #[cfg_attr(
        not(build_for_sandbox),
        ignore = "registry should be isolated in sandbox tests. use `cargo xtask sandbox run --test` instead."
    )]
    fn list_valid_package_fonts_returns_empty_for_missing_package() {
        with_registry_test(|reg_key| {
            let pkg_id = test_package_id("missing-list");
            let fonts = reg_key.list_valid_package_fonts(&pkg_id).unwrap();
            assert!(fonts.is_empty());
        });
    }

    #[test]
    #[cfg_attr(
        not(build_for_sandbox),
        ignore = "registry should be isolated in sandbox tests. use `cargo xtask sandbox run --test` instead."
    )]
    fn unregister_package_fonts_ignores_missing_package() {
        with_registry_test(|reg_key| {
            let pkg_id = test_package_id("missing-unregister");
            reg_key.unregister_package_fonts(&pkg_id).unwrap();
        });
    }

    #[test]
    #[cfg_attr(
        not(build_for_sandbox),
        ignore = "registry should be isolated in sandbox tests. use `cargo xtask sandbox run --test` instead."
    )]
    fn register_list_and_unregister_package_fonts_roundtrip() {
        with_registry_test(|reg_key| {
            let pkg_id = test_package_id("roundtrip");
            let mut expected_fonts = [
                registered_font(r"C:\path\to\example-font-a.ttf"),
                registered_font(r"C:\path\to\example-font-b.ttc"),
            ];
            expected_fonts.sort();

            reg_key
                .register_package_fonts(&pkg_id, &expected_fonts)
                .unwrap();

            let mut actual_fonts = reg_key.list_valid_package_fonts(&pkg_id).unwrap();
            actual_fonts.sort();
            assert_eq!(actual_fonts, expected_fonts);

            reg_key.unregister_package_fonts(&pkg_id).unwrap();

            let fonts_after_unregister = reg_key.list_valid_package_fonts(&pkg_id).unwrap();
            assert!(fonts_after_unregister.is_empty());
        });
    }

    #[test]
    #[cfg_attr(
        not(build_for_sandbox),
        ignore = "registry should be isolated in sandbox tests. use `cargo xtask sandbox run --test` instead."
    )]
    fn list_valid_package_fonts_ignores_non_string_value() {
        with_registry_test(|reg_key| {
            let pkg_id = test_package_id("invalid-value");
            let valid_value_name = reg_value_name(reg_key.app_id, &pkg_id, "valid-font.ttf");
            let invalid_value_name = reg_value_name(reg_key.app_id, &pkg_id, "invalid-font.ttf");

            let key_path = reg_key.key_path.as_ref();
            let key = CURRENT_USER.create(key_path).unwrap();
            key.set_string(&valid_value_name, r"C:\path\to\valid-font.ttf")
                .unwrap();
            key.set_u32(&invalid_value_name, 42).unwrap();

            let actual_fonts = reg_key.list_valid_package_fonts(&pkg_id).unwrap();
            let expected_fonts = [registered_font(r"C:\path\to\valid-font.ttf")];

            assert_eq!(actual_fonts, expected_fonts);
        });
    }

    #[test]
    #[cfg_attr(
        not(build_for_sandbox),
        ignore = "registry should be isolated in sandbox tests. use `cargo xtask sandbox run --test` instead."
    )]
    fn register_and_unregister_package_fonts_ignore_unrelated_values() {
        with_registry_test(|reg_key| {
            let pkg_id = test_package_id("ignore-unrelated");
            let other_pkg_id = test_package_id("ignore-unrelated-other");
            let mut expected_fonts = [
                registered_font(r"C:\path\to\target-font-a.ttf"),
                registered_font(r"C:\path\to\target-font-b.ttf"),
            ];
            expected_fonts.sort();
            let other_font = registered_font(r"C:\path\to\other-font.ttf");
            let other_value_name = other_font.reg_value_name(reg_key.app_id, &other_pkg_id);

            let key_path = reg_key.key_path.as_ref();
            let key = CURRENT_USER.create(key_path).unwrap();
            key.set_u32("Unrelated Value", 42).unwrap();
            key.set_value(&other_value_name, &other_font.reg_value())
                .unwrap();

            reg_key
                .register_package_fonts(&pkg_id, &expected_fonts)
                .unwrap();

            let mut actual_fonts = reg_key.list_valid_package_fonts(&pkg_id).unwrap();
            actual_fonts.sort();

            assert_eq!(actual_fonts, expected_fonts);

            reg_key.unregister_package_fonts(&pkg_id).unwrap();

            let value_names = list_value_names(reg_key);
            assert!(value_names.contains(&"Unrelated Value".to_owned()));
            assert!(value_names.contains(&other_value_name));
            assert!(!value_names.iter().any(|value_name| {
                strip_value_name_prefix(reg_key.app_id, &pkg_id, value_name).is_some()
            }));
        });
    }

    #[test]
    #[cfg_attr(
        not(build_for_sandbox),
        ignore = "registry should be isolated in sandbox tests. use `cargo xtask sandbox run --test` instead."
    )]
    fn unregister_package_fonts_removes_invalid_matching_value() {
        with_registry_test(|reg_key| {
            let pkg_id = test_package_id("invalid-unregister");
            let value_name = reg_value_name(reg_key.app_id, &pkg_id, "broken-font.ttf");

            let key_path = reg_key.key_path.as_ref();
            let key = CURRENT_USER.create(key_path).unwrap();
            key.set_u32(&value_name, 42).unwrap();

            reg_key.unregister_package_fonts(&pkg_id).unwrap();
            assert!(list_value_names(reg_key).is_empty());
        });
    }

    #[test]
    #[cfg_attr(
        not(build_for_sandbox),
        ignore = "registry should be isolated in sandbox tests. use `cargo xtask sandbox run --test` instead."
    )]
    fn unregister_package_fonts_keeps_other_package_values() {
        with_registry_test(|reg_key| {
            let pkg_id_v1 = PackageId::from_str("registry-test-cleanup-keep-keys@0.1.0").unwrap();
            let pkg_id_v2 =
                PackageId::from_str("registry-test-cleanup-keep-keys-other@0.2.0").unwrap();
            let fonts_v1 = [registered_font(r"C:\path\to\v1\example-font.ttf")];
            let fonts_v2 = [registered_font(r"C:\path\to\v2\example-font.ttf")];

            reg_key
                .register_package_fonts(&pkg_id_v1, &fonts_v1)
                .unwrap();
            reg_key
                .register_package_fonts(&pkg_id_v2, &fonts_v2)
                .unwrap();

            reg_key.unregister_package_fonts(&pkg_id_v1).unwrap();

            let fonts_v1 = reg_key.list_valid_package_fonts(&pkg_id_v1).unwrap();
            let fonts_v2 = reg_key.list_valid_package_fonts(&pkg_id_v2).unwrap();
            assert!(fonts_v1.is_empty());
            assert!(!fonts_v2.is_empty());
        });
    }

    #[test]
    #[cfg_attr(
        not(build_for_sandbox),
        ignore = "registry should be isolated in sandbox tests. use `cargo xtask sandbox run --test` instead."
    )]
    fn register_package_fonts_errors_when_package_already_exists() {
        with_registry_test(|reg_key| {
            let pkg_id = test_package_id("duplicate-register");
            let fonts = [registered_font(r"C:\path\to\example-font.ttf")];

            let key_path = reg_key.key_path.as_ref();
            let key = CURRENT_USER.create(key_path).unwrap();
            key.set_value(
                fonts[0].reg_value_name(reg_key.app_id, &pkg_id),
                &fonts[0].reg_value(),
            )
            .unwrap();

            let err = reg_key.register_package_fonts(&pkg_id, &fonts).unwrap_err();
            match err {
                RegistryError::PackageRegistryValuesAlreadyExist {
                    pkg_id: err_pkg_id,
                    key_path: err_key_path,
                } => {
                    assert_eq!(err_pkg_id, pkg_id);
                    assert_eq!(err_key_path, key_path);
                }
                other => panic!("unexpected registry error: {other:?}"),
            }
        });
    }
}
