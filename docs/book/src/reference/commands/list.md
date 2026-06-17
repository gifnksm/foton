# list

List installed packages.

## Usage

```text
foton list [OPTIONS]
```

## Options

### `--show-incomplete`

Include packages left by incomplete installs, uninstalls, or updates.

Without this option, only packages in the `installed` state are shown, and each
line includes the activation state.
With this option, each line includes the installation state, and installed
packages also include the activation state.

### `--no-confirm`

Skip interactive confirmation prompts.

### `--exit-on-lock`

Exit immediately if the package database is locked by another operation.

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

Without `--show-incomplete`, each line contains a package name, version, and
activation state:

```text
example-font@1.2.3 (active)
```

With `--show-incomplete`, each line includes the installation state, and
installed packages also include the activation state:

```text
example-font@1.2.3 (installed, active)
another-font@0.1.0 (incomplete-install)
```

## Notes

- `list` reads the local package database and does not access package registries.
- Use `info` when you want more than the package name, version, and recorded
  states.

## Related pages

- [Managing Installed Packages](../../guide/basic/manage-installed-packages.md)
- [activate command reference](activate.md)
- [deactivate command reference](deactivate.md)
- [info command reference](info.md)
- [repair command reference](repair.md)
