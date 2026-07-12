# uninstall

Uninstall packages recorded in the local package database.

## Usage

```text
foton uninstall [OPTIONS] <PACKAGE>...
```

## Arguments

### `<PACKAGE>`

Package names, optionally with an exact version as `<package-name>@<version>`.

## Global options

### `--exit-on-lock`

Exit immediately if the package database is locked by another operation.

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

```console
foton uninstall <package-name>@<version>
```

## Notes

- If an uninstall does not complete cleanly, use `repair` to clean up any
  packages it leaves behind.
- `uninstall` operates on packages recorded in the local package database and
  does not access package registries.

## Related pages

- [Managing Installed Packages](../../guide/basic/manage-installed-packages.md)
- [repair command reference](repair.md)
- [list command reference](list.md)
- [info command reference](info.md)
