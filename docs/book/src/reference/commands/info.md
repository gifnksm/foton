# info

Show detailed information about packages recorded in the local package database.

## Usage

```text
foton info [OPTIONS] <PACKAGE>...
```

## Arguments

### `<PACKAGE>`

Package names, optionally with an exact version as `<package-name>@<version>`.

## Options

### `--no-confirm`

Skip interactive confirmation prompts.

### `--exit-on-lock`

Exit immediately if the package database is locked by another operation.

### `--warnings-as-errors`

Treat warnings as errors, causing the command to fail if any warning is emitted.

## Examples

```console
foton info <package-name>
```

```console
foton info <package-name>@<version>
```

## Output

`info` prints detailed metadata for matching packages recorded in the local package database, including:

- package name and display name
- version, installation state, and activation state
- description, aliases, faces, homepage, repository, and license
- source URLs, hashes, and include or exclude patterns

If a package name matches multiple packages recorded in the local package
database, `info` prints all of them.
This can include packages left by incomplete operations.

## Notes

- `info` reads the local package database and does not search package registries.
- If no package recorded in the local package database matches the specified
  package name, the command fails.
- Use `repair` when you want to clean up packages left by incomplete
  operations.
- Use `search` when you want to inspect packages that are available in a
  package registry but not yet installed.

## Related pages

- [Managing Installed Packages](../../guide/basic/manage-installed-packages.md)
- [list command reference](list.md)
- [repair command reference](repair.md)
- [search command reference](search.md)
