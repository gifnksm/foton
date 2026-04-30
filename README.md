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

A simple font package manager for Windows using GitHub Releases.

Manage fonts as packages using versioned manifests that describe how each package version is installed from GitHub Releases.

> [!WARNING]
> `foton` is still in early development.
> The current README describes the planned CLI and overall behavior, and details may change freely until the first release.

## Features

* Install font packages from GitHub Releases using versioned manifests
* Track installed packages with version and hash
* Update packages safely between manifest-defined versions
* Clean uninstall

## Installation

There are several ways to install `foton`.
Choose the one that best fits your needs.

### Pre-built binaries

Executable binaries are available for download on the [GitHub Release page].

You can also install the binary with [`cargo-binstall`].

```console
# Install pre-built binary
$ cargo binstall foton
```

[GitHub Release page]: https://github.com/gifnksm/foton/releases/
[`cargo-binstall`]: https://github.com/cargo-bins/cargo-binstall

### Build from source using Rust

To build `foton` from source, you need the Rust toolchain installed.
If you do not have Rust yet, follow [this guide](https://www.rust-lang.org/tools/install).

Once Rust is installed, you can build and install `foton` with:

```console
# Install released version
$ cargo install foton

# Install latest version
$ cargo install --git https://github.com/gifnksm/foton.git foton
```

## Usage

### Install

Install a font package from the manifest registry:

```bash
foton install <package spec>
```

Example:

```bash
foton install hackgen
foton install hackgen@2.10.0
```

This command is intended to:

* Resolve the package spec to a versioned manifest
* Read the manifest for the selected package version
* Fetch the GitHub release and asset specified by that manifest
* Extract the font files specified by that manifest
* Install the fonts into the system
* Record package and file metadata in the local database

Each installable package version is defined by a single resolved manifest.
The manifest fully specifies how that version is fetched and installed.

---

### Update

Update all installed font packages:

```bash
foton update
```

Update a specific package:

```bash
foton update <package name>
```

Example:

```bash
foton update hackgen
```

The planned behavior is:

* Check whether a newer package version is available in the manifest registry
* Resolve the target version to its own manifest
* Compare the installed package version with the target package version
* Install the new version according to the target manifest
* Remove files from the old version that are no longer needed

Updates are defined as transitions between manifest-defined package versions, rather than by inferring behavior directly from arbitrary GitHub release assets.
If file hashes differ unexpectedly, the tool may ask for confirmation.

---

### Uninstall

Remove an installed font package:

```bash
foton uninstall <package name>
```

Example:

```bash
foton uninstall hackgen
```

This command is intended to:

* Remove installed font files
* Unregister the fonts from the system
* Remove the package entry from the database

---

### List

List installed font packages:

```bash
foton list
```

Example output:

```text
hackgen   v2.10.0
inter     v4.0.2
```

---

### Info

Show details of an installed font package:

```bash
foton info <package name>
```

Example:

```bash
foton info hackgen
```

Expected output may include:

* Package name
* Package ID (`name@version`)
* Source repository
* Installed package version
* Manifest identifier
* File list
* Hashes

---

### Dry Run

Preview changes without applying:

```bash
foton update --dry-run
```

---

## Data Storage

`foton` is expected to maintain a local database of installed font packages.

The database is expected to store:

* Package name
* Package ID (`name@version`)
* Source repository (`user/repo`)
* Installed package version
* Manifest identifier for the installed version
* Installed font files and hashes

---

## Current scope

* Support is currently planned for `.ttf` and `.otf` files
* Installation and update are driven by versioned manifests
* Each installable package version is represented by one resolved manifest
* The current design uses GitHub Releases as the package payload source
* Manifest generation and authoring ergonomics are currently out of scope

---

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
