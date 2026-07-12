# deactivate

Deactivate installed packages.

## Usage

```text
foton deactivate [OPTIONS] <PACKAGE>...
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
foton deactivate <package-name>
```

```console
foton deactivate <package-name>@<version>
```

```console
foton deactivate <package-name-1> <package-name-2>
```

## Notes

- `deactivate` operates on packages recorded in the local package database and
  does not access package registries.
- Use `repair` when a package command such as `install` or `activate` does
  not complete cleanly and leaves packages in incomplete states.

## Related pages

- [Managing Installed Packages](../../guide/basic/manage-installed-packages.md)
- [activate command reference](activate.md)
- [repair command reference](repair.md)
- [info command reference](info.md)
