# install

Install packages from package registries or manifest files.

## Usage

```text
foton install [OPTIONS] [<PACKAGE>...] [--manifest <MANIFEST>...]
```

## Arguments

### `<PACKAGE>`

Package names, optionally with an exact version as `<package-name>@<version>`.

At least one `<PACKAGE>` or `--manifest` is required.

## Options

### `--manifest <MANIFEST>`

Install packages defined in the given manifest files.

This option can be specified multiple times.

### `--registry <REGISTRY_ID>`

Package registry IDs to resolve packages from.

Use a comma-separated list such as `--registry local,foton`.
This option applies only to packages installed by `<PACKAGE>`.

### `--pre-release`

Allow installing pre-release versions when resolving packages from registries.

Without this option, versions with a suffix such as `1.2.3-rc-1` are ignored
unless an exact version is specified.
This option applies only to packages installed by `<PACKAGE>`.

### `--no-activate`

Do not activate the installed packages.
Use `foton activate` later if you want to make them active manually.

## Global options

### `--exit-on-lock`

Exit immediately if the package database is locked by another operation.

### `--no-confirm`

Skip interactive confirmation prompts.

### `--warnings-as-errors`

Treat warnings as errors, causing the command to fail if any warning is emitted.

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

- Use `foton update` when you want to look for newer versions of already
  installed packages in package registries.
- If an install does not complete cleanly, use `repair` to clean up any
  packages it leaves behind.
- Installing from a manifest file is useful for local testing before adding it
  to a package registry.

## Related pages

- [Installing and Updating Packages](../../guide/basic/install-update-packages.md)
- [activate command reference](activate.md)
- [repair command reference](repair.md)
- [Writing a Package Manifest](../../guide/advanced/write-package-manifest.md)
