use std::env;

use winresource::WindowsResource;

const SANDBOX_ENV_NAMES: &[&str] = &["GITHUB_ACTIONS", "FOTON_SANDBOX_TEST"];
const SANDBOX_CFG_NAME: &str = "build_for_sandbox";

fn main() {
    assert_eq!(
        env::var("CARGO_CFG_TARGET_OS").as_deref(),
        Ok("windows"),
        "foton must be built for Windows"
    );
    println!("cargo::rerun-if-changed=build.rs");
    apply_sandbox_cfg();
    embed_windows_version_resource();
}

fn build_for_sandbox() -> bool {
    for name in SANDBOX_ENV_NAMES {
        println!("cargo::rerun-if-env-changed={name}");
    }
    SANDBOX_ENV_NAMES
        .iter()
        .any(|name| env::var_os(name).is_some_and(|value| !value.is_empty()))
}

fn apply_sandbox_cfg() {
    println!("cargo::rustc-check-cfg=cfg({SANDBOX_CFG_NAME})");
    if build_for_sandbox() {
        println!("cargo::rustc-cfg={SANDBOX_CFG_NAME}");
    }
}

fn embed_windows_version_resource() {
    println!("cargo::rerun-if-changed=Cargo.toml");
    let res = WindowsResource::new();
    res.compile().unwrap();
}
