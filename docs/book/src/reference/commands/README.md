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

- [`install`](install.md)
- [`update`](update.md)
- [`uninstall`](uninstall.md)
- [`activate`](activate.md)
- [`deactivate`](deactivate.md)
- [`repair`](repair.md)
- [`list`](list.md)
- [`info`](info.md)
- [`search`](search.md)
- [`manifest`](manifest/README.md)
- [`font`](font/README.md)
