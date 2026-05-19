# install

Install packages from package registries or manifest files.

## Usage

Install from package registries:

```text
foton install [OPTIONS] [<PACKAGE>...]
```

Install from local manifest files:

```text
foton install [OPTIONS] --manifest <MANIFEST>...
```

## Arguments

### `<PACKAGE>`

Package names, optionally with an exact version as `<package-name>@<version>`.

Required unless `--manifest` is specified.
This cannot be used together with `--manifest`.

## Options

### `--manifest <MANIFEST>`

Install packages defined in the given manifest files.

This option can be specified multiple times.
It cannot be used together with `--registry`, `--pre-release` or `<PACKAGE>`.

### `--registry <REGISTRY_ID>`

Package registry IDs to resolve packages from.

Use a comma-separated list such as `--registry local,foton`.
This option is only available when installing by `<PACKAGE>`.

### `--no-confirm`

Skip interactive confirmation prompts.

### `--warnings-as-errors`

Treat warnings as errors, causing the command to fail if any warning is emitted.

### `--pre-release`

Allow installing pre-release versions when resolving packages from registries.

Without this option, versions with a suffix such as `1.2.3-rc-1` are ignored
unless an exact version is specified.
This cannot be used together with `--manifest`.

## Examples

```console
foton install <package-name>
```

```console
foton install <package-name>@<version>
```

```console
foton install --registry <registry-id-1>,<registry-id-2> <package-name>
```

```console
foton install --manifest <manifest-path>
```

## Notes

- If the selected packages are already installed and nothing needs to change,
  `foton` reports that and exits without modifying the system.
- When installing by package name, if a matching package is already installed
  locally, `foton` keeps that installed package as the selected result.
  Use `foton update` when you want to look for newer versions in package registries.
- If multiple selected package registries provide a matching package, `install` does
  not choose one automatically; it fails and asks you to disambiguate.
- Installing from a manifest file is useful for local testing before adding it
  to a package registry.

## Related pages

- [Installing and Updating Packages](../../guide/basic/install-update-packages.md)
- [Writing a Package Manifest](../../guide/advanced/write-package-manifest.md)
