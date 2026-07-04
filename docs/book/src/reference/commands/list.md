# list

List packages recorded in the local package database.

## Usage

```text
foton list [OPTIONS]
```

## Options

### `--no-confirm`

Skip interactive confirmation prompts.

### `--exit-on-lock`

Exit immediately if the package database is locked by another operation.

### `--warnings-as-errors`

Treat warnings as errors, causing the command to fail if any warning is emitted.

### `--format <FORMAT>`

Select the output format.

- **Default**: `text`
- **Possible Values**: `text`, `jsonl`

## Examples

Show packages recorded in the local package database:

```console
foton list
```

Show packages as JSON Lines:

```console
foton list --format jsonl
```

## Output

`list` prints packages recorded in the local package database.
In text output, each line contains a package name, version, installation state,
and activation state:

```text
example-font@1.2.3 (installed, active)
```

With `--format jsonl`, `list` writes one JSON object per listed package
in JSON Lines format.

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
