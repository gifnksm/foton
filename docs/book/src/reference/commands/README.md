# Command Reference

This section documents the `foton` command-line interface.

## Command structure

The general command format is:

```text
foton [GLOBAL_OPTIONS] <COMMAND> [COMMAND_OPTIONS] [ARGS...]
```

Run `foton --help` to see the complete command-line help.

## Global options

### `--no-confirm`

Skip interactive confirmation prompts.
This is most useful with commands that change installed packages, such as
`install`, `update`, and `uninstall`.

### `--warnings-as-errors`

Treat warnings as errors, causing the command to fail if any warning is emitted.

## Commands

- [`install`](install.md)
- [`update`](update.md)
- [`uninstall`](uninstall.md)
- [`list`](list.md)
- [`info`](info.md)
- [`search`](search.md)
- [`manifest`](manifest/README.md)
