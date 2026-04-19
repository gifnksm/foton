use std::{ffi::OsString, fmt::Display};

use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows_core::HSTRING;
use windows_registry::{CURRENT_USER, Value};

use crate::{package::PackageId, util::path::AbsolutePath};

const USER_FONTS_REGISTRY_BASE_KEY: &str = r"Software\Microsoft\Windows NT\CurrentVersion\Fonts";

fn assert_registry_path_segment<S>(kind: &str, segment: S)
where
    S: Display,
{
    let segment = segment.to_string();
    assert!(
        !segment.contains(['\\', '\0']) && !segment.chars().any(char::is_control),
        "invalid registry path segment for {kind}: {segment:?}"
    );
}

fn app_registry_key(app_id: &str) -> String {
    assert_registry_path_segment("app id", app_id);
    format!(r"{USER_FONTS_REGISTRY_BASE_KEY}\{app_id}")
}

fn package_registry_key(app_id: &str, pkg_id: &PackageId) -> String {
    assert_registry_path_segment("package name", pkg_id.name());
    format!(r"{}\{}", app_registry_key(app_id), pkg_id.name())
}

fn package_version_registry_key(app_id: &str, pkg_id: &PackageId) -> String {
    assert_registry_path_segment("package version", pkg_id.version());
    format!(
        r"{}\{}",
        package_registry_key(app_id, pkg_id),
        pkg_id.version()
    )
}

fn err_is_not_found(err: &windows_result::Error) -> bool {
    err.code() == ERROR_FILE_NOT_FOUND.to_hresult()
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum RegistryError {
    #[display("failed to open registry key: {path}")]
    OpenRegistryKey {
        path: String,
        #[error(source)]
        source: windows_core::Error,
    },
    #[display("failed to create registry key: {path}")]
    CreateRegistryKey {
        path: String,
        #[error(source)]
        source: windows_core::Error,
    },
    #[display("failed to remove registry key: {path}")]
    RemoveRegistryKey {
        path: String,
        #[error(source)]
        source: windows_core::Error,
    },
    #[display("failed to enumerate subkeys of registry key: {path}")]
    EnumerateSubkeys {
        path: String,
        #[error(source)]
        source: windows_core::Error,
    },
    #[display("failed to enumerate values of registry key: {path}")]
    EnumerateValues {
        path: String,
        #[error(source)]
        source: windows_core::Error,
    },
    #[display("failed to set registry value for font `{title}`: {path}")]
    SetFontValue {
        path: String,
        title: String,
        #[error(source)]
        source: windows_core::Error,
    },
    #[display("registry key for package version already exists: {path}")]
    PackageKeyAlreadyExists { path: String },
    #[display("invalid font entry found in registry key: {path}")]
    InvalidEntryFound {
        path: String,
        #[error(source)]
        source: Box<RegisteredFontError>,
    },
    #[display("failed to prune empty registry key: {path}")]
    PruneEmptyKey {
        path: String,
        #[error(source)]
        source: Box<Self>,
    },
}

pub(crate) fn list_registered_package_fonts(
    app_id: &str,
    pkg_id: &PackageId,
) -> Result<Vec<RegisteredFont>, RegistryError> {
    let path = package_version_registry_key(app_id, pkg_id);

    let key = match CURRENT_USER.open(&path) {
        Ok(key) => key,
        Err(err) if err_is_not_found(&err) => {
            return Ok(vec![]);
        }
        Err(source) => {
            let path = path.clone();
            return Err(RegistryError::OpenRegistryKey { path, source });
        }
    };

    key.values()
        .map_err(|source| {
            let path = path.clone();
            RegistryError::EnumerateValues { path, source }
        })?
        .map(|(name, value)| {
            RegisteredFont::from_reg(name, value).map_err(|source| {
                let path = path.clone();
                let source = Box::new(source);
                RegistryError::InvalidEntryFound { path, source }
            })
        })
        .collect()
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
    let path = package_version_registry_key(app_id, pkg_id);

    match CURRENT_USER.open(&path) {
        Ok(_) => return Err(RegistryError::PackageKeyAlreadyExists { path }),
        Err(err) if err_is_not_found(&err) => {}
        Err(source) => return Err(RegistryError::OpenRegistryKey { path, source }),
    }

    let key = CURRENT_USER.create(&path).map_err(|source| {
        let path = path.clone();
        RegistryError::CreateRegistryKey { path, source }
    })?;

    for font in fonts {
        let font = font.as_ref();
        key.set_value(font.reg_name(), &font.reg_value())
            .map_err(|source| {
                let path = path.clone();
                let title = font.title().to_string();
                RegistryError::SetFontValue {
                    path,
                    title,
                    source,
                }
            })?;
    }

    Ok(())
}

pub(crate) fn unregister_package_fonts(
    app_id: &str,
    pkg_id: &PackageId,
) -> Result<(), RegistryError> {
    let path = package_version_registry_key(app_id, pkg_id);

    if let Err(err) = CURRENT_USER.remove_tree(&path) {
        if err_is_not_found(&err) {
            return Ok(());
        }
        return Err(RegistryError::RemoveRegistryKey { path, source: err });
    }

    for parent_path in [
        package_registry_key(app_id, pkg_id),
        app_registry_key(app_id),
    ] {
        remove_key_if_empty(&parent_path).map_err(|source| {
            let path = parent_path.clone();
            let source = Box::new(source);
            RegistryError::PruneEmptyKey { path, source }
        })?;
    }

    Ok(())
}

fn remove_key_if_empty(path: &str) -> Result<(), RegistryError> {
    let key = match CURRENT_USER.open(path) {
        Ok(key) => key,
        Err(err) if err_is_not_found(&err) => {
            return Ok(());
        }
        Err(source) => {
            let path = path.to_owned();
            return Err(RegistryError::OpenRegistryKey { path, source });
        }
    };

    let has_subkeys = key
        .keys()
        .map_err(|source| {
            let path = path.to_owned();
            RegistryError::EnumerateSubkeys { path, source }
        })?
        .next()
        .is_some();
    if has_subkeys {
        return Ok(());
    }

    let has_values = key
        .values()
        .map_err(|source| {
            let path = path.to_owned();
            RegistryError::EnumerateValues { path, source }
        })?
        .next()
        .is_some();
    if has_values {
        return Ok(());
    }

    if let Err(err) = CURRENT_USER.remove_tree(path) {
        if err_is_not_found(&err) {
            return Ok(());
        }
        let path = path.to_owned();
        return Err(RegistryError::RemoveRegistryKey { path, source: err });
    }

    Ok(())
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum RegisteredFontError {
    #[display("registered font path for `{name}` has invalid value type")]
    InvalidFontPathValueType {
        name: String,
        #[error(source)]
        source: windows_core::Error,
    },
    #[display("registered font path for `{name}` is not an absolute path: {path}", path = path.display())]
    FontPathIsNotAbsolute { name: String, path: OsString },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RegisteredFont {
    title: String,
    path: AbsolutePath,
}

impl AsRef<RegisteredFont> for RegisteredFont {
    fn as_ref(&self) -> &RegisteredFont {
        self
    }
}

impl RegisteredFont {
    pub(crate) fn new<T, P>(title: T, path: P) -> Self
    where
        T: Into<String>,
        P: Into<AbsolutePath>,
    {
        let name = title.into();
        let path = path.into();
        Self { title: name, path }
    }

    pub(crate) fn title(&self) -> &str {
        &self.title
    }

    pub(crate) fn path(&self) -> &AbsolutePath {
        &self.path
    }

    fn from_reg(reg_name: String, reg_value: Value) -> Result<Self, RegisteredFontError> {
        let path = HSTRING::try_from(reg_value)
            .map_err(|source| {
                let name = reg_name.clone();
                RegisteredFontError::InvalidFontPathValueType { name, source }
            })?
            .to_os_string();
        let path = AbsolutePath::new(&path).ok_or_else(|| {
            let name = reg_name.clone();
            RegisteredFontError::FontPathIsNotAbsolute { name, path }
        })?;
        Ok(Self::new(reg_name, path))
    }

    fn reg_name(&self) -> &str {
        &self.title
    }

    fn reg_value(&self) -> Value {
        Value::from(&HSTRING::from(self.path.as_path()))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        process,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use semver::{BuildMetadata, Version};

    use crate::package::PackageName;

    use super::*;

    struct RegistryTestGuard {
        app_id: String,
    }

    impl RegistryTestGuard {
        fn new() -> Self {
            static TEST_ID: AtomicUsize = AtomicUsize::new(0);
            let app_id = format!(
                "io.github.gifnksm.foton.test.{}.{}",
                process::id(),
                TEST_ID.fetch_add(1, Ordering::Relaxed)
            );
            cleanup_app_root(&app_id);
            Self { app_id }
        }
    }

    impl Drop for RegistryTestGuard {
        fn drop(&mut self) {
            cleanup_app_root(&self.app_id);
        }
    }

    fn with_registry_test<T>(f: impl FnOnce(&str) -> T) -> T {
        let guard = RegistryTestGuard::new();
        f(&guard.app_id)
    }

    fn test_package_id(name: &str) -> PackageId {
        let name = PackageName::new(format!("registry-test-{name}")).unwrap();
        let version = Version::parse(&format!("0.1.0+pid-{}", process::id())).unwrap();
        PackageId::new(name, version)
    }

    fn cleanup_app_root(app_id: &str) {
        let path = app_registry_key(app_id);
        if let Err(err) = CURRENT_USER.remove_tree(&path) {
            assert!(
                err_is_not_found(&err),
                "failed to clean up app registry root `{path}`: {err:?}"
            );
        }
    }

    fn list_key_names(path: &str) -> Vec<String> {
        let key = match CURRENT_USER.open(path) {
            Ok(key) => key,
            Err(err) if err_is_not_found(&err) => {
                return vec![];
            }
            Err(err) => panic!("failed to open registry key `{path}`: {err:?}"),
        };

        key.keys()
            .unwrap_or_else(|err| {
                panic!("failed to enumerate subkeys of registry key `{path}`: {err:?}")
            })
            .collect()
    }

    #[test]
    #[cfg_attr(
        not(build_for_sandbox),
        ignore = "registry should be isolated in sandbox tests. use `cargo xtask sandbox run --test` instead."
    )]
    fn list_registered_package_fonts_returns_empty_for_missing_package() {
        with_registry_test(|app_id| {
            let pkg_id = test_package_id("missing-list");

            let entries = list_registered_package_fonts(app_id, &pkg_id)
                .expect("listing missing package fonts should succeed");

            assert!(entries.is_empty());
        });
    }

    #[test]
    #[cfg_attr(
        not(build_for_sandbox),
        ignore = "registry should be isolated in sandbox tests. use `cargo xtask sandbox run --test` instead."
    )]
    fn unregister_package_fonts_ignores_missing_package() {
        with_registry_test(|app_id| {
            let pkg_id = test_package_id("missing-unregister");

            unregister_package_fonts(app_id, &pkg_id)
                .expect("unregistering missing package fonts should succeed");
        });
    }

    #[test]
    #[cfg_attr(
        not(build_for_sandbox),
        ignore = "registry should be isolated in sandbox tests. use `cargo xtask sandbox run --test` instead."
    )]
    fn register_list_and_unregister_package_fonts_roundtrip() {
        with_registry_test(|app_id| {
            let pkg_id = test_package_id("roundtrip");

            let expected_entries = [
                RegisteredFont::new(
                    "Example Font A (TrueType)",
                    AbsolutePath::new(r"C:\path\to\example-font-a.ttf").unwrap(),
                ),
                RegisteredFont::new(
                    "Example Font B (TrueType)",
                    AbsolutePath::new(r"C:\path\to\example-font-b.ttc").unwrap(),
                ),
            ];

            register_package_fonts(app_id, &pkg_id, &expected_entries)
                .expect("registering package fonts should succeed");

            let mut actual_entries = list_registered_package_fonts(app_id, &pkg_id)
                .expect("listing registered package fonts should succeed");
            actual_entries.sort_by(|lhs, rhs| lhs.title().cmp(rhs.title()));

            let mut expected_entries = expected_entries;
            expected_entries.sort_by(|lhs, rhs| lhs.title().cmp(rhs.title()));

            assert_eq!(actual_entries.len(), expected_entries.len());
            for (actual, expected) in actual_entries.iter().zip(expected_entries.iter()) {
                assert_eq!(actual.title(), expected.title());
                assert_eq!(actual.path(), expected.path());
            }

            unregister_package_fonts(app_id, &pkg_id)
                .expect("unregistering registered package fonts should succeed");

            let entries_after_unregister = list_registered_package_fonts(app_id, &pkg_id)
                .expect("listing package fonts after unregister should succeed");
            assert!(entries_after_unregister.is_empty());
        });
    }

    #[test]
    #[cfg_attr(
        not(build_for_sandbox),
        ignore = "registry should be isolated in sandbox tests. use `cargo xtask sandbox run --test` instead."
    )]
    fn list_registered_package_fonts_errors_on_non_string_value() {
        with_registry_test(|app_id| {
            let pkg_id = test_package_id("invalid-value");

            let path = package_version_registry_key(app_id, &pkg_id);
            let key = CURRENT_USER
                .create(&path)
                .expect("failed to create test registry key");
            key.set_u32("Invalid Font", 42)
                .expect("failed to write invalid registry value");

            let err = list_registered_package_fonts(app_id, &pkg_id)
                .expect_err("listing package fonts with non-string value should fail");
            match err {
                RegistryError::InvalidEntryFound {
                    path: err_path,
                    source,
                } => {
                    assert_eq!(err_path, path);
                    match *source {
                        RegisteredFontError::InvalidFontPathValueType { name, .. } => {
                            assert_eq!(name, "Invalid Font");
                        }
                        other @ RegisteredFontError::FontPathIsNotAbsolute { .. } => {
                            panic!("unexpected registered font error: {other:?}")
                        }
                    }
                }
                other => panic!("unexpected registry error: {other:?}"),
            }
        });
    }

    #[test]
    #[cfg_attr(
        not(build_for_sandbox),
        ignore = "registry should be isolated in sandbox tests. use `cargo xtask sandbox run --test` instead."
    )]
    fn unregister_package_fonts_removes_empty_parent_keys() {
        with_registry_test(|app_id| {
            let pkg_id = test_package_id("cleanup-empty-parents");

            register_package_fonts(
                app_id,
                &pkg_id,
                [RegisteredFont::new(
                    "Example Font (TrueType)",
                    AbsolutePath::new(r"C:\path\to\example-font.ttf").unwrap(),
                )],
            )
            .expect("registering package fonts should succeed");

            assert_eq!(
                list_key_names(&app_registry_key(app_id)),
                [pkg_id.name().as_str()]
            );
            assert_eq!(
                list_key_names(&package_registry_key(app_id, &pkg_id)),
                [pkg_id.version().to_string()]
            );

            unregister_package_fonts(app_id, &pkg_id)
                .expect("unregistering registered package fonts should succeed");

            assert!(list_key_names(&app_registry_key(app_id)).is_empty());
            let err = CURRENT_USER
                .open(app_registry_key(app_id))
                .expect_err("app registry key should be removed when it becomes empty");
            assert!(err_is_not_found(&err));
        });
    }

    #[test]
    #[cfg_attr(
        not(build_for_sandbox),
        ignore = "registry should be isolated in sandbox tests. use `cargo xtask sandbox run --test` instead."
    )]
    fn unregister_package_fonts_keeps_non_empty_parent_keys() {
        with_registry_test(|app_id| {
            let pkg_id_v1 = test_package_id("cleanup-keep-parents");
            let mut v2 = pkg_id_v1.version().clone();
            v2.build = BuildMetadata::new(&format!("{}-other", v2.build.as_str())).unwrap();
            let pkg_id_v2 = PackageId::new(pkg_id_v1.name(), v2);

            let entries = [RegisteredFont::new(
                "Example Font (TrueType)",
                AbsolutePath::new(r"C:\path\to\example-font.ttf").unwrap(),
            )];

            register_package_fonts(app_id, &pkg_id_v1, &entries)
                .expect("registering first package fonts should succeed");
            register_package_fonts(app_id, &pkg_id_v2, &entries)
                .expect("registering second package fonts should succeed");

            unregister_package_fonts(app_id, &pkg_id_v1)
                .expect("unregistering first package fonts should succeed");

            assert_eq!(
                list_key_names(&app_registry_key(app_id)),
                [pkg_id_v1.name().as_str()]
            );
            assert_eq!(
                list_key_names(&package_registry_key(app_id, &pkg_id_v1)),
                [pkg_id_v2.version().to_string()]
            );
        });
    }

    #[test]
    #[cfg_attr(
        not(build_for_sandbox),
        ignore = "registry should be isolated in sandbox tests. use `cargo xtask sandbox run --test` instead."
    )]
    fn register_package_fonts_errors_when_package_version_already_exists() {
        with_registry_test(|app_id| {
            let pkg_id = test_package_id("duplicate-register");
            let entries = [RegisteredFont::new(
                "Example Font (TrueType)",
                AbsolutePath::new(r"C:\path\to\example-font.ttf").unwrap(),
            )];

            let path = package_version_registry_key(app_id, &pkg_id);
            let key = CURRENT_USER
                .create(&path)
                .expect("failed to create existing test registry key");
            key.set_value(entries[0].reg_name(), &entries[0].reg_value())
                .expect("failed to seed existing registry value");

            let err = register_package_fonts(app_id, &pkg_id, &entries)
                .expect_err("registering duplicate package version should fail");
            match err {
                RegistryError::PackageKeyAlreadyExists { path: err_path } => {
                    assert_eq!(err_path, path);
                }
                other => panic!("unexpected registry error: {other:?}"),
            }
        });
    }
}
