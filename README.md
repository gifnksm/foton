<!-- cargo-sync-rdme title [[ -->
# foton
<!-- cargo-sync-rdme ]] -->
<!-- cargo-sync-rdme badge [[ -->
[![Maintenance: actively-developed](https://img.shields.io/badge/maintenance-actively--developed-brightgreen.svg?style=flat-square)](https://doc.rust-lang.org/cargo/reference/manifest.html#the-badges-section)
[![License: MIT OR Apache-2.0](https://img.shields.io/crates/l/foton.svg?style=flat-square)](#license)
[![crates.io](https://img.shields.io/crates/v/foton.svg?logo=rust&style=flat-square)](https://crates.io/crates/foton)
[![Rust: ^1.88.0](https://img.shields.io/badge/rust-^1.88.0-93450a.svg?logo=rust&style=flat-square)](https://doc.rust-lang.org/cargo/reference/manifest.html#the-rust-version-field)
[![GitHub Actions: CI](https://img.shields.io/github/actions/workflow/status/gifnksm/foton/ci.yml.svg?label=CI&logo=github&style=flat-square)](https://github.com/gifnksm/foton/actions/workflows/ci.yml)
[![Codecov](https://img.shields.io/codecov/c/github/gifnksm/foton.svg?label=codecov&logo=codecov&style=flat-square)](https://codecov.io/gh/gifnksm/foton)
<!-- cargo-sync-rdme ]] -->

A simple font package manager for Windows using GitHub Releases.

Manage fonts as packages. Install and update directly from GitHub Releases.

> [!WARNING]
> `foton` is still in early development.
> The current README describes the planned CLI and overall behavior, and details may change freely until the first release.

## Features

* Install fonts from GitHub Releases
* Track installed fonts with version and hash
* Update fonts safely
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

Install a font from a GitHub repository:

```bash
foton install <user>/<repo>
```

Example:

```bash
foton install yuru7/HackGen
```

This command is intended to:

* Fetch the latest release
* Download the release assets
* Extract `.ttf` and `.otf` files
* Install the fonts into the system
* Record metadata in the local database

---

### Update

Update all installed fonts:

```bash
foton update
```

Update a specific font:

```bash
foton update <name>
```

Example:

```bash
foton update HackGen
```

The planned behavior is:

* Check the latest GitHub release
* Compare it with the installed version
* Install a new version if one is available
* Remove the old version

If file hashes differ unexpectedly, the tool may ask for confirmation.

---

### Uninstall

Remove an installed font:

```bash
foton uninstall <name>
```

Example:

```bash
foton uninstall HackGen
```

This command is intended to:

* Remove installed font files
* Unregister the fonts from the system
* Remove the entry from the database

---

### List

List installed fonts:

```bash
foton list
```

Example output:

```text
HackGen   v2.10.0
Inter     v4.0.2
```

---

### Info

Show details of an installed font:

```bash
foton info <name>
```

Example:

```bash
foton info HackGen
```

Expected output may include:

* Source repository
* Installed version
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

`foton` is expected to maintain a local database of installed fonts.

The database is expected to store:

* Source repository (`user/repo`)
* Installed version (release tag)
* Font files and hashes

---

## Current scope

* Support is currently planned for `.ttf` and `.otf` files
* The current design assumes that all font files in a release are installed
* The current design is based solely on GitHub Releases

---

## Minimum supported Rust version (MSRV)

The minimum supported Rust version is **Rust 1.88.0**.
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
