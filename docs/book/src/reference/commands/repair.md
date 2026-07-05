# repair

Clean up packages left in incomplete states when commands such as `install`
or `activate` do not complete cleanly.

`repair` only performs cleanup. It does not retry or resume those commands.

## Usage

```text
foton repair [OPTIONS] [<PACKAGE>...]
```

## Arguments

### `<PACKAGE>`

Package names, optionally with an exact version as `<package-name>@<version>`.

If not specified, every package that needs cleanup will be cleaned up.

## Global options

### `--exit-on-lock`

Exit immediately if the package database is locked by another operation.

### `--no-confirm`

Skip interactive confirmation prompts.

### `--warnings-as-errors`

Treat warnings as errors, causing the command to fail if any warning is emitted.

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

- If a selected package does not need cleanup, `foton` reports that there is
  nothing to do.
- If cleanup cannot be completed, the package may remain in the local package
  database so that you can retry `repair` later.

## Related pages

- [Managing Installed Packages](../../guide/basic/manage-installed-packages.md)
- [install command reference](install.md)
- [update command reference](update.md)
- [uninstall command reference](uninstall.md)
- [list command reference](list.md)
- [info command reference](info.md)
