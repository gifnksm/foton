# activate

Activate installed packages.

## Usage

```text
foton activate [OPTIONS] <PACKAGE>...
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
foton activate <package-name>
```

```console
foton activate <package-name>@<version>
```

```console
foton activate <package-name-1> <package-name-2>
```

## Notes

- `activate` operates on packages recorded in the local package database and
  does not access package registries.
- Only one version of a package name can be active at a time.
  Activating one version deactivates any other active version of the same
  package name.
- If the selected package is already active, `foton` reports that there is
  nothing to do.
- If no installed package matches the specified package, the command fails.
- If a package name matches multiple installed versions, `activate` does not
  choose one automatically; specify an exact package ID such as
  `<package-name>@<version>`.

## Related pages

- [Managing Installed Packages](../../guide/basic/manage-installed-packages.md)
- [deactivate command reference](deactivate.md)
- [install command reference](install.md)
- [info command reference](info.md)
