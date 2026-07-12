# update

Update installed packages from package registries.

## Usage

```text
foton update [OPTIONS] [<PACKAGE>...]
```

## Arguments

### `<PACKAGE>`

Package names, optionally with an exact version as `<package-name>@<version>`.

If not specified, `update` checks installed packages for newer versions.

## Options

### `--registry <REGISTRY_ID>`

Package registry IDs to resolve packages from.

Use a comma-separated list such as `--registry local,foton`.

### `--pre-release`

Allow updating to pre-release versions when resolving packages from registries.

Without this option, versions with a suffix such as `1.2.3-rc-1` are ignored.

## Global options

### `--exit-on-lock`

Exit immediately if the package database is locked by another operation.

### `--no-confirm`

Skip interactive confirmation prompts.

### `--warnings-as-errors`

Treat warnings as errors, causing the command to fail if any warning is emitted.

## Examples

```console
foton update
```

```console
foton update <package-name>
```

```console
foton update --registry <registry-id-1>,<registry-id-2> <package-name>
```

## Notes

- If an update does not complete cleanly, use `repair` to clean up any
  packages it leaves behind.
- Updating a package installs the newer version without automatically removing
  older installed versions.
- Update resolution uses package names and package registry IDs, not manifest files.

## Related pages

- [Installing and Updating Packages](../../guide/basic/install-update-packages.md)
- [repair command reference](repair.md)
- [Package Registry Reference](../package-registry.md)
