# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

<!-- next-header -->

## [Unreleased] - ReleaseDate

### Added

* Embed dependency information in binaries published on GitHub Releases using
  [`cargo auditable`], enabling dependency inspection and vulnerability
  scanning.

[`cargo auditable`]: https://github.com/rust-secure-code/cargo-auditable

### Changed

* Publish crates through crates.io trusted publishing and attest GitHub release
  assets

### Security

* Update the transitive dependency `ctutils` from 0.4.2 to 0.4.5 to address
  [RUSTSEC-2026-0385] (non-constant-time mask generation and `u128`
  comparisons).
* Update the dependency `zip` from 8.6.0 to 9.0.5 to address [RUSTSEC-2026-0367]
  (preservation of SUID/SGID bits during extraction), [RUSTSEC-2026-0368]
  (creation of symlinks targeting paths outside the extraction directory), and
  [RUSTSEC-2026-0369] (permission changes through symlinks without confining the
  target).

[RUSTSEC-2026-0367]: https://rustsec.org/advisories/RUSTSEC-2026-0367
[RUSTSEC-2026-0368]: https://rustsec.org/advisories/RUSTSEC-2026-0368
[RUSTSEC-2026-0369]: https://rustsec.org/advisories/RUSTSEC-2026-0369
[RUSTSEC-2026-0385]: https://rustsec.org/advisories/RUSTSEC-2026-0385

## [0.1.3] - 2026-08-29

### Added

* Add `-V` and `--version`
* Embed Windows version information in `foton.exe`

## [0.1.2] - 2026-07-31

### Fixed

* Use `zip` release assets for `cargo binstall` on Windows
* Statically link the CRT in `aarch64-pc-windows-msvc` builds

## [0.1.1] - 2026-07-31

### Changed

* Distribute Windows release archives as `.zip` instead of `.tar.gz`

## [0.1.0] - 2026-07-12

* First release

<!-- next-url -->
[Unreleased]: https://github.com/gifnksm/foton/compare/v0.1.3...HEAD
[0.1.3]: https://github.com/gifnksm/foton/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/gifnksm/foton/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/gifnksm/foton/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/gifnksm/foton/commits/v0.1.0
