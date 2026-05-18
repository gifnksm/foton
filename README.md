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

## Documentation

* [**Latest**](https://foton.readthedocs.io/en/latest/) — documentation for the development version
* [**Stable**](https://foton.readthedocs.io/en/stable/) — documentation for the latest release

## Installation

For detailed installation instructions, see the [installation guide](https://foton.readthedocs.io/en/stable/guide/install-foton.html).

Quick install options:

```console
# Install the latest released version from pre-built binaries
$ cargo binstall foton

# Install the latest released version with Cargo
$ cargo install foton

# Install the current development version with Cargo
$ cargo install --git https://github.com/gifnksm/foton.git foton
```

You can also download pre-built binaries manually from the [GitHub Releases page].

[GitHub Releases page]: https://github.com/gifnksm/foton/releases/

## Usage

For tutorials and day-to-day workflows, see the [basic usage guide](https://foton.readthedocs.io/en/stable/guide/basic/).
For the full CLI reference, see the [command reference](https://foton.readthedocs.io/en/stable/reference/commands/).

Common commands:

```console
# Install one or more font packages:
foton install <package-name>...

# Update all installed packages:
foton update

# Search registries for packages:
foton search <query>...
```

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
