# list

List installed packages.

## Usage

```text
foton list [OPTIONS]
```

## Options

### `--show-incomplete`

Include packages left by incomplete installs, uninstalls, or updates.

Without this option, only packages in the `installed` state are shown.
With this option, leftover packages are shown with states such as
`install-incomplete` and `uninstall-incomplete`.

### `--no-confirm`

Skip interactive confirmation prompts.

### `--warnings-as-errors`

Treat warnings as errors, causing the command to fail if any warning is emitted.

## Examples

Show installed packages:

```console
foton list
```

Show installed packages together with leftover packages from incomplete
installs, uninstalls, or updates, and their states:

```console
foton list --show-incomplete
```

## Output

Without `--show-incomplete`, each line contains a package name and version:

```text
example-font@1.2.3
```

With `--show-incomplete`, each line also includes the package state:

```text
example-font@1.2.3 (installed)
another-font@0.1.0 (install-incomplete)
```

## Notes

- `list` reads the local package database and does not access package registries.
- Use `info` when you want more than the package name, version, and state.

## Related pages

- [Managing Installed Packages](../../guide/basic/manage-installed-packages.md)
- [info command reference](info.md)
- [repair command reference](repair.md)
