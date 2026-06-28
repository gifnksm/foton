# font list

List fonts managed by foton.

## Usage

```text
foton font list [OPTIONS]
```

## Options

### `--no-confirm`

Skip interactive confirmation prompts.

### `--show-system-fonts`

Include all system fonts recognized by Windows.

### `--show-user-fonts`

Include all user fonts recognized by Windows.

### `--warnings-as-errors`

Treat warnings as errors, causing the command to fail if any warning is
emitted.

## Examples

Show fonts managed by foton:

```console
foton font list
```

Also show system fonts recognized by Windows:

```console
foton font list --show-system-fonts
```

Also show user fonts recognized by Windows:

```console
foton font list --show-user-fonts
```

Show fonts from all supported sources:

```console
foton font list --show-system-fonts --show-user-fonts
```

## Output

`font list` groups fonts by source.
Within each source, each line shows a font family followed by the faces
recognized for that family.

Example:

```text
Package example-font@1.2.3:
  - Example Font (Bold, Regular)
System Fonts:
  - Example Sans (Regular)
User Fonts:
  - Example Serif (Italic)
```

## Notes

- By default, `font list` shows fonts attributed to foton-managed packages.
- `--show-system-fonts` adds system fonts recognized by Windows.
- `--show-user-fonts` adds user fonts recognized by Windows.
- `font list` reads the font set recognized by Windows and classifies each
  visible font by the location of its backing file when possible.

## Related pages

- [font command reference](README.md)
- [info command reference](../info.md)
