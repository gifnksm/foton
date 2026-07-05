# font list

List fonts managed by foton.

## Usage

```text
foton font list [OPTIONS]
```

## Options

### `--include-system-fonts`

Also include all system fonts recognized by Windows.

### `--include-user-fonts`

Also include all user fonts recognized by Windows.

### `--format <FORMAT>`

Select the output format.

- **Default**: `text`
- **Possible Values**: `text`, `jsonl`

## Global options

### `--exit-on-lock`

Exit immediately if the package database is locked by another operation.

### `--no-confirm`

Skip interactive confirmation prompts.

### `--warnings-as-errors`

Treat warnings as errors, causing the command to fail if any warning is emitted.

## Examples

Show fonts managed by foton:

```console
foton font list
```

Also show system fonts recognized by Windows:

```console
foton font list --include-system-fonts
```

Also show user fonts recognized by Windows:

```console
foton font list --include-user-fonts
```

Also show system and user fonts:

```console
foton font list --include-system-fonts --include-user-fonts
```

Show fonts as JSON Lines:

```console
foton font list --format jsonl
```

## Output

By default, `font list` prints text output grouped by font locations.
Within each group, each line shows a font family followed by the faces
recognized for that family.

Example:

```text
Fonts from Package example-font@1.2.3:
  - Example Font (Bold, Regular)
Fonts from System Font Directory:
  - Example Sans (Regular)
Fonts from User Font Directories:
  - Example Serif (Italic)
```

With `--format jsonl`, `font list` writes one JSON object per visible font face
in JSON Lines format.

## Notes

- By default, `font list` shows fonts attributed to foton-managed packages.
- `--include-system-fonts` adds system fonts recognized by Windows.
- `--include-user-fonts` adds user fonts recognized by Windows.
- `font list` reads the font set recognized by Windows and classifies each
  visible font by the location of its backing file when possible.

## Related pages

- [font command reference](README.md)
- [info command reference](../info.md)
