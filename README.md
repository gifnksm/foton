<!-- cargo-sync-rdme title [[ -->
# foton
<!-- cargo-sync-rdme ]] -->
<!-- cargo-sync-rdme badge [[ -->
[![Maintenance: actively-developed](https://img.shields.io/badge/maintenance-actively--developed-brightgreen.svg?style=flat-square)](https://doc.rust-lang.org/cargo/reference/manifest.html#the-badges-section)
[![License: MIT OR Apache-2.0](https://img.shields.io/crates/l/foton.svg?style=flat-square)](#license)
[![crates.io](https://img.shields.io/crates/v/foton.svg?logo=rust&style=flat-square)](https://crates.io/crates/foton)
[![Rust: ^1.95.0](https://img.shields.io/badge/rust-^1.95.0-93450a.svg?logo=rust&style=flat-square)](https://doc.rust-lang.org/cargo/reference/manifest.html#the-rust-version-field)
[![GitHub Actions: CI](https://img.shields.io/github/actions/workflow/status/gifnksm/foton/ci.yml.svg?label=CI&logo=github&style=flat-square)](https://github.com/gifnksm/foton/actions/workflows/ci.yml)
[![Codecov](https://img.shields.io/codecov/c/github/gifnksm/foton.svg?label=codecov&logo=codecov&style=flat-square&component=foton)](https://codecov.io/gh/gifnksm/foton)
<!-- cargo-sync-rdme ]] -->

`foton` is a font manager for Windows that lets you install, update, and uninstall fonts for the current user without administrator privileges.

## Features

* Install, update, and uninstall fonts for the current user
* No administrator privileges required
* Install fonts from the default package registry or your own custom registries
* Use simple TOML manifests to define font packages from existing online distributions; see [`foton-registry`] for examples

[`foton-registry`]: https://github.com/gifnksm/foton-registry

## Installation

There are several ways to install `foton`.
Choose the one that best fits your needs.

### Pre-built binaries

Pre-built binaries are available on the [GitHub Releases page].

You can also install one with [`cargo-binstall`].

```console
# Install pre-built binary
$ cargo binstall foton
```

[GitHub Releases page]: https://github.com/gifnksm/foton/releases/
[`cargo-binstall`]: https://github.com/cargo-bins/cargo-binstall

### Install with Cargo

To install `foton` with Cargo, you need the Rust toolchain.
If you do not have Rust yet, follow [this guide](https://www.rust-lang.org/tools/install).

Once Rust is installed, run one of the following commands:

```console
# Install released version
$ cargo install foton

# Install latest version
$ cargo install --git https://github.com/gifnksm/foton.git foton
```

## Usage

For the full command reference, run `foton --help`.

```bash
# Install one or more font packages:
foton install <package-specifier>...

# Update all installed packages:
foton update
# Update specific packages:
foton update <package-specifier>...

# Uninstall one or more installed packages:
foton uninstall <package-specifier>...

# List installed packages:
foton list

# Search registries for packages:
foton search <query>...

# Show details for one or more installed packages:
foton info <package-specifier>...
```

A package specifier can be written in one of the following forms:

* `<name>` - package name
* `<name>@<version>` - package ID with an explicit version

For example: `hackgen` or `hackgen@2.10.0`.

Use `--registry` to choose which package registries to search, for example:
`foton install --registry local,foton hackgen`

## Minimum supported Rust version (MSRV)

The minimum supported Rust version is **Rust 1.95.0**.
At least the last 3 versions of stable Rust are supported at any given time.

While the crate is in a pre-release state (`0.x.x`), its MSRV may be bumped in a patch release.
Once a crate has reached 1.x, any MSRV bump will be accompanied by a new minor version.

## License

This project is licensed under either of

* Apache License, Version 2.0
   ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
* MIT license
   ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.

See [CONTRIBUTING.md](CONTRIBUTING.md).
