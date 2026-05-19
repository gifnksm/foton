# search

Search packages in package registries.

## Usage

```text
foton search [OPTIONS] <QUERY>...
```

## Arguments

### `<QUERY>`

Search query terms.

All specified query terms must match within the same package metadata field.

## Options

### `--registry <REGISTRY_ID>`

Package registry IDs to search.

Use a comma-separated list such as `--registry local,foton`.

### `--limit <LIMIT>`

Maximum number of matching packages to show.

### `--no-confirm`

Skip interactive confirmation prompts.

### `--warnings-as-errors`

Treat warnings as errors, causing the command to fail if any warning is emitted.

### `--pre-release`

Allow matching pre-release versions when searching packages in registries.

Without this option, versions with a suffix such as `1.2.3-rc-1` are ignored.

## Examples

```console
foton search <query>
```

```console
foton search --limit 20 <query>
```

```console
foton search --registry <registry-id-1>,<registry-id-2> <query>
```

## Output

Each result shows the package name with version and the package registry ID in brackets.
If the package has a description, it is printed on the next line.

Example:

```text
example-font@1.2.3 [example]
  Example package description
```

## Notes

- By default, `search` looks at the latest version of each package in each
  selected package registry without considering pre-release versions.
  Use `--pre-release` if you want pre-release versions to be included.
- Search can match package names, display names, aliases, faces, and
  descriptions.
- If no matching packages are found, the command fails.

## Related pages

- [Discovering Packages](../../guide/basic/discover-packages.md)
- [install](install.md)
