# repair

Clean up packages in the local package database that were left by incomplete
installs, uninstalls, or updates.

Installed packages are not changed by `repair`. `repair` only performs cleanup; it does not resume an interrupted install or update.

## Usage

```text
foton repair [OPTIONS] [<PACKAGE>...]
```

## Arguments

### `<PACKAGE>`

Package names, optionally with an exact version as `<package-name>@<version>`.

If not specified, every package that needs cleanup will be cleaned up.

## Options

### `--no-confirm`

Skip interactive confirmation prompts

### `--warnings-as-errors`

Treat warnings as errors, causing the command to fail if any warning is emitted

## Examples

```console
foton repair
```

```console
foton repair <package-name>
```

```console
foton repair <package-name>@<version>
```

## Notes

- Installed packages are not changed by `repair`.
- If a selected package is already in a consistent state, `foton` reports that
  there is nothing to do.
- If cleanup cannot be completed, the package may remain in the local package
  database so that you can retry `repair` later.

## Related pages

- [Managing Installed Packages](../../guide/basic/manage-installed-packages.md)
- [install command reference](install.md)
- [update command reference](update.md)
- [uninstall command reference](uninstall.md)
- [list command reference](list.md)
- [info command reference](info.md)
