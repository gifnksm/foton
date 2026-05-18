# list

List installed packages.

## Usage

```text
foton list [OPTIONS]
```

## Options

### `--show-pending`

Include packages in pending states such as `pending-install` and
`pending-uninstall`.

Without this option, only packages in the `installed` state are shown.
If you see packages in pending states, it usually means that an earlier
install or uninstall was interrupted or failed before it finished cleanly.

### `--no-confirm`

Skip interactive confirmation prompts.

### `--warnings-as-errors`

Treat warnings as errors, causing the command to fail if any warning is emitted.

## Examples

Show installed packages:

```console
foton list
```

Show installed packages together with packages in pending states and their states:

```console
foton list --show-pending
```

## Output

Without `--show-pending`, each line contains a package name and version:

```text
example-font@1.2.3
```

With `--show-pending`, each line also includes the package state:

```text
example-font@1.2.3 (installed)
another-font@0.1.0 (pending-install)
```

## Notes

- `list` reads the local package database and does not access package registries.
- Use `info` when you want more than the package name, version, and state.

## Related pages

- [Managing Installed Packages](../../guide/basic/manage-installed-packages.md)
- [info](info.md)
