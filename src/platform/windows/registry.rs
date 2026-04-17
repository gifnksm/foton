use std::path::PathBuf;

use color_eyre::eyre::{self, WrapErr as _};
use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows_registry::{CURRENT_USER, HSTRING, Value};

use crate::package::PackageId;

const USER_FONTS_REGISTRY_BASE_KEY: &str = r"Software\Microsoft\Windows NT\CurrentVersion\Fonts";

fn assert_registry_path_segment(kind: &str, segment: &str) {
    assert!(
        !segment.contains(['\\', '\0']) && !segment.chars().any(char::is_control),
        "invalid registry path segment for {kind}: {segment:?}"
    );
}

fn app_registry_key(app_id: &str) -> String {
    assert_registry_path_segment("app id", app_id);
    format!(r"{USER_FONTS_REGISTRY_BASE_KEY}\{app_id}")
}

fn package_registry_key(app_id: &str, pkgid: &PackageId) -> String {
    assert_registry_path_segment("package name", &pkgid.name);
    format!(r"{}\{}", app_registry_key(app_id), pkgid.name)
}

fn package_version_registry_key(app_id: &str, pkgid: &PackageId) -> String {
    assert_registry_path_segment("package version", &pkgid.version);
    format!(r"{}\{}", package_registry_key(app_id, pkgid), pkgid.version)
}

fn err_is_not_found(err: &windows_result::Error) -> bool {
    err.code() == ERROR_FILE_NOT_FOUND.to_hresult()
}

#[derive(Debug, Clone)]
pub(crate) struct FontEntry {
    pub(crate) name: String,
    pub(crate) path: PathBuf,
}

impl FontEntry {
    fn from_reg(name: String, path: Value) -> eyre::Result<Self> {
        let path = HSTRING::try_from(path)
            .wrap_err_with(|| format!("failed to convert registry value `{name}` to string"))?;
        let path = PathBuf::from(path.to_os_string());
        Ok(Self { name, path })
    }

    #[must_use]
    fn reg_name(&self) -> &str {
        &self.name
    }

    #[must_use]
    fn reg_value(&self) -> Value {
        Value::from(&HSTRING::from(self.path.as_path()))
    }
}

pub(crate) fn list_registered_package_fonts(
    app_id: &str,
    pkgid: &PackageId,
) -> eyre::Result<Vec<FontEntry>> {
    let path = package_version_registry_key(app_id, pkgid);

    let key = match CURRENT_USER.open(&path) {
        Ok(key) => key,
        Err(err) if err_is_not_found(&err) => {
            return Ok(vec![]);
        }
        Err(err) => {
            return Err(err).wrap_err_with(|| {
                format!("failed to open registry key `{path}` for package `{pkgid}`")
            });
        }
    };

    key.values()
        .wrap_err_with(|| {
            format!("failed to read registry value of key `{path}` for package `{pkgid}`")
        })?
        .map(|(font_name, font_path)| {
            let entry = FontEntry::from_reg(font_name, font_path)?;
            Ok(entry)
        })
        .collect()
}

pub(crate) fn register_package_fonts(
    app_id: &str,
    pkgid: &PackageId,
    entries: &[FontEntry],
) -> eyre::Result<()> {
    let path = package_version_registry_key(app_id, pkgid);

    match CURRENT_USER.open(&path) {
        Ok(_) => {
            eyre::bail!("registry key `{path}` already exists for package `{pkgid}`");
        }
        Err(err) if err_is_not_found(&err) => {}
        Err(err) => {
            return Err(err).wrap_err_with(|| {
                format!("failed to check registry key `{path}` for package `{pkgid}`")
            });
        }
    }

    let key = CURRENT_USER.create(&path).wrap_err_with(|| {
        format!("failed to create registry key `{path}` for package `{pkgid}`")
    })?;

    for entry in entries {
        key.set_value(entry.reg_name(), &entry.reg_value())
            .wrap_err_with(|| {
                format!(
                    "failed to set registry value for key `{path}` and font `{}`",
                    entry.name
                )
            })?;
    }

    Ok(())
}

pub(crate) fn unregister_package_fonts(app_id: &str, pkgid: &PackageId) -> eyre::Result<()> {
    let path = package_version_registry_key(app_id, pkgid);

    if let Err(err) = CURRENT_USER.remove_tree(&path) {
        if err_is_not_found(&err) {
            return Ok(());
        }
        return Err(err).wrap_err_with(|| {
            format!("failed to delete registry key `{path}` for package `{pkgid}`")
        });
    }

    for parent_path in [
        package_registry_key(app_id, pkgid),
        app_registry_key(app_id),
    ] {
        remove_key_if_empty(&parent_path).wrap_err_with(|| {
            format!("failed to clean up parent registry key `{parent_path}` for package `{pkgid}`")
        })?;
    }

    Ok(())
}

fn remove_key_if_empty(path: &str) -> eyre::Result<()> {
    let key = match CURRENT_USER.open(path) {
        Ok(key) => key,
        Err(err) if err_is_not_found(&err) => {
            return Ok(());
        }
        Err(err) => {
            return Err(err).wrap_err_with(|| format!("failed to open registry key `{path}`"));
        }
    };

    let has_subkeys = key
        .keys()
        .wrap_err_with(|| format!("failed to enumerate subkeys of registry key `{path}`"))?
        .next()
        .is_some();
    if has_subkeys {
        return Ok(());
    }

    let has_values = key
        .values()
        .wrap_err_with(|| format!("failed to enumerate values of registry key `{path}`"))?
        .next()
        .is_some();
    if has_values {
        return Ok(());
    }

    if let Err(err) = CURRENT_USER.remove_tree(path) {
        if err_is_not_found(&err) {
            return Ok(());
        }
        return Err(err).wrap_err_with(|| format!("failed to delete empty registry key `{path}`"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        process,
        sync::atomic::{AtomicUsize, Ordering},
    };

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
        PackageId {
            name: format!("registry-test-{name}"),
            version: format!("pid-{}", process::id()),
        }
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
            let pkgid = test_package_id("missing-list");

            let entries = list_registered_package_fonts(app_id, &pkgid)
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
            let pkgid = test_package_id("missing-unregister");

            unregister_package_fonts(app_id, &pkgid)
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
            let pkgid = test_package_id("roundtrip");

            let expected_entries = vec![
                FontEntry {
                    name: "Example Font A (TrueType)".to_string(),
                    path: PathBuf::from(r"C:\path\to\example-font-a.ttf"),
                },
                FontEntry {
                    name: "Example Font B (TrueType)".to_string(),
                    path: PathBuf::from(r"C:\path\to\example-font-b.ttf"),
                },
            ];

            register_package_fonts(app_id, &pkgid, &expected_entries)
                .expect("registering package fonts should succeed");

            let mut actual_entries = list_registered_package_fonts(app_id, &pkgid)
                .expect("listing registered package fonts should succeed");
            actual_entries.sort_by(|lhs, rhs| lhs.name.cmp(&rhs.name));

            let mut expected_entries = expected_entries;
            expected_entries.sort_by(|lhs, rhs| lhs.name.cmp(&rhs.name));

            assert_eq!(actual_entries.len(), expected_entries.len());
            for (actual, expected) in actual_entries.iter().zip(expected_entries.iter()) {
                assert_eq!(actual.name, expected.name);
                assert_eq!(actual.path, expected.path);
            }

            unregister_package_fonts(app_id, &pkgid)
                .expect("unregistering registered package fonts should succeed");

            let entries_after_unregister = list_registered_package_fonts(app_id, &pkgid)
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
            let pkgid = test_package_id("invalid-value");

            let path = package_version_registry_key(app_id, &pkgid);
            let key = CURRENT_USER
                .create(&path)
                .expect("failed to create test registry key");
            key.set_u32("Invalid Font", 42)
                .expect("failed to write invalid registry value");

            let err = list_registered_package_fonts(app_id, &pkgid)
                .expect_err("listing package fonts with non-string value should fail");
            let message = format!("{err:?}");
            assert!(message.contains("failed to convert registry value `Invalid Font`"));
        });
    }

    #[test]
    #[cfg_attr(
        not(build_for_sandbox),
        ignore = "registry should be isolated in sandbox tests. use `cargo xtask sandbox run --test` instead."
    )]
    fn unregister_package_fonts_removes_empty_parent_keys() {
        with_registry_test(|app_id| {
            let pkgid = test_package_id("cleanup-empty-parents");

            register_package_fonts(
                app_id,
                &pkgid,
                &[FontEntry {
                    name: "Example Font (TrueType)".to_string(),
                    path: PathBuf::from(r"C:\path\to\example-font.ttf"),
                }],
            )
            .expect("registering package fonts should succeed");

            assert_eq!(
                list_key_names(&app_registry_key(app_id)),
                vec![pkgid.name.clone()]
            );
            assert_eq!(
                list_key_names(&package_registry_key(app_id, &pkgid)),
                vec![pkgid.version.clone()]
            );

            unregister_package_fonts(app_id, &pkgid)
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
            let pkgid_v1 = test_package_id("cleanup-keep-parents");
            let pkgid_v2 = PackageId {
                name: pkgid_v1.name.clone(),
                version: format!("{}-other", pkgid_v1.version),
            };

            let entries = [FontEntry {
                name: "Example Font (TrueType)".to_string(),
                path: PathBuf::from(r"C:\path\to\example-font.ttf"),
            }];

            register_package_fonts(app_id, &pkgid_v1, &entries)
                .expect("registering first package fonts should succeed");
            register_package_fonts(app_id, &pkgid_v2, &entries)
                .expect("registering second package fonts should succeed");

            unregister_package_fonts(app_id, &pkgid_v1)
                .expect("unregistering first package fonts should succeed");

            assert_eq!(
                list_key_names(&app_registry_key(app_id)),
                vec![pkgid_v1.name.clone()]
            );
            assert_eq!(
                list_key_names(&package_registry_key(app_id, &pkgid_v1)),
                vec![pkgid_v2.version.clone()]
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
            let pkgid = test_package_id("duplicate-register");
            let entries = [FontEntry {
                name: "Example Font (TrueType)".to_string(),
                path: PathBuf::from(r"C:\path\to\example-font.ttf"),
            }];

            let path = package_version_registry_key(app_id, &pkgid);
            let key = CURRENT_USER
                .create(&path)
                .expect("failed to create existing test registry key");
            key.set_value(entries[0].reg_name(), &entries[0].reg_value())
                .expect("failed to seed existing registry value");

            let err = register_package_fonts(app_id, &pkgid, &entries)
                .expect_err("registering duplicate package version should fail");
            let message = format!("{err:?}");
            assert!(message.contains("already exists"));
        });
    }
}
