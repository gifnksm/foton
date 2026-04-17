use std::env;

const SANDBOX_ENV_NAMES: &[&str] = &["GITHUB_ACTIONS", "FOTON_SANDBOX_TEST"];
const SANDBOX_CFG_NAME: &str = "build_for_sandbox";

fn main() {
    println!("cargo::rustc-check-cfg=cfg(build_for_sandbox)");
    println!("cargo::rerun-if-changed=build.rs");
    for name in SANDBOX_ENV_NAMES {
        println!("cargo::rerun-if-env-changed={name}");
    }

    let build_for_sandbox = SANDBOX_ENV_NAMES
        .iter()
        .any(|name| env::var_os(name).is_some_and(|value| !value.is_empty()));

    if build_for_sandbox {
        println!("cargo::rustc-cfg={SANDBOX_CFG_NAME}");
    }
}
