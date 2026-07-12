# Command Reference

This section documents the `foton` command-line interface.

## Command structure

The general command format is:

```text
foton [GLOBAL_OPTIONS] <COMMAND> [COMMAND_OPTIONS] [ARGS...]
```

Run `foton --help` to see the complete command-line help.

## Global options

### `--exit-on-lock`

Exit immediately if the package database is locked by another operation.

### `--no-confirm`

Skip interactive confirmation prompts.

### `--warnings-as-errors`

Treat warnings as errors, causing the command to fail if any warning is emitted.

## Commands

- [`install`](install.md): Install packages from package registries or manifest files
- [`update`](update.md): Update installed packages from package registries
- [`uninstall`](uninstall.md): Uninstall packages recorded in the local package database
- [`activate`](activate.md): Activate installed packages
- [`deactivate`](deactivate.md): Deactivate installed packages
- [`repair`](repair.md): Clean up packages left in incomplete states
- [`list`](list.md): List packages recorded in the local package database
- [`info`](info.md): Show detailed information about packages recorded in the local package database
- [`search`](search.md): Search packages in package registries
- [`manifest`](manifest/README.md): Work with package manifest files
- [`font`](font/README.md): Work with fonts managed by foton
