# manifest check

Validate a manifest file for installation errors and quality warnings.

## Usage

```text
foton manifest check [OPTIONS] <MANIFEST>
```

## Arguments

### `<MANIFEST>`

Path to the manifest file to validate.

## Options

### `--no-confirm`

Skip interactive confirmation prompts.

### `--warnings-as-errors`

Treat warnings as errors, causing the command to fail if any warning is emitted.

## What the command checks

`manifest check` reads the manifest and then stages it as if it were going to be
installed.
This includes downloading and examining the source archives or files described
by the manifest.

The command reports:

- installation errors that would prevent the package from being installed
- quality warnings for common authoring mistakes

## Common warnings

Warnings can include:

- missing `display-name`
- missing `description`
- missing `license`
- duplicate display names or face names after normalization
- `files` rules that match nothing
- `ignore` rules that match nothing
- `glob` entries in `files`
- font-like files that match neither `files` nor `ignore`

## Examples

```console
foton manifest check <manifest-path>
```

Treat warnings as errors:

```console
foton --warnings-as-errors manifest check <manifest-path>
```

## Notes

- This command is primarily intended for package authors.
- Because the command fetches sources, network access may be required.
- A manifest that parses successfully can still fail `manifest check` if the
  sources are invalid or the selected files do not install correctly.

## Related pages

- [Writing a Package Manifest](../../../guide/advanced/write-package-manifest.md)
- [Package Manifest Reference](../../package-manifest.md)
- [manifest command reference](README.md)
