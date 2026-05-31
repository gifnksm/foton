# update

Update installed packages from package registries.

## Usage

```text
foton update [OPTIONS] [<PACKAGE>...]
```

## Arguments

### `<PACKAGE>`

Package names, optionally with an exact version as `<package-name>@<version>`.

If not specified, all installed packages will be updated if possible.
When an exact version is specified, `update` selects the matching installed
package first, then looks for a newer version of the same package name.

## Options

### `--registry <REGISTRY_ID>`

Package registry IDs to resolve packages from.

Use a comma-separated list such as `--registry local,foton`.

### `--no-confirm`

Skip interactive confirmation prompts.

### `--warnings-as-errors`

Treat warnings as errors, causing the command to fail if any warning is emitted.

### `--pre-release`

Allow updating to pre-release versions when resolving packages from registries.

Without this option, versions with a suffix such as `1.2.3-rc-1` are ignored.

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

- If no newer version is available for the selected packages, `foton` reports
  that they are already up to date.
- If an update does not complete cleanly, use `repair` to clean up any
  packages it leaves behind.
- Update resolution uses package names and package registry IDs, not manifest files.
- If multiple selected package registries provide newer versions of the same package,
  `update` does not choose one automatically; it fails and asks you to
  disambiguate.

## Related pages

- [Installing and Updating Packages](../../guide/basic/install-update-packages.md)
- [repair command reference](repair.md)
- [Package Registry Reference](../package-registry.md)
