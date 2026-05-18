# uninstall

Uninstall packages recorded in the local package database.

## Usage

```text
foton uninstall [OPTIONS] <PACKAGE>...
```

## Arguments

### `<PACKAGE>`

Package names, optionally with an exact version as `<package-name>@<version>`.

## Options

### `--no-confirm`

Skip interactive confirmation prompts.

### `--warnings-as-errors`

Treat warnings as errors, causing the command to fail if any warning is emitted.

## Examples

```console
foton uninstall <package-name>
```

```console
foton uninstall <package-name-1> <package-name-2>
```

## Notes

- If the selected package is already absent, `foton` reports that there is
  nothing to do.
- `uninstall` operates on packages recorded in the local package database and
  does not access package registries.
- `uninstall` can also target packages still recorded in the local package
  database after an interrupted or failed install or uninstall.

## Related pages

- [Managing Installed Packages](../../guide/basic/manage-installed-packages.md)
- [list](list.md)
- [info](info.md)
