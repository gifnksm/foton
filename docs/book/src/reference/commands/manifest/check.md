# manifest check

Validate manifest files for installation errors and quality warnings.

## Usage

```text
foton manifest check [OPTIONS] <MANIFEST>...
```

## Arguments

### `<MANIFEST>`

Paths to the manifest files to validate.

## Options

### `--no-confirm`

Skip interactive confirmation prompts.

### `--no-source-checks`

Skip checks that require downloading and examining the source archives or files.

### `--registry-root <REGISTRY_ROOT>`

Treat the given manifest files as belonging to the package registry rooted at this directory.

### `--warnings-as-errors`

Treat warnings as errors, causing the command to fail if any warning is emitted.

## What the command checks

By default, `manifest check` reads the manifest and then stages it as if it
were going to be installed.
This includes downloading and examining the source archives or files described
by the manifest.
Use `--no-source-checks` to skip those source-dependent checks.

The command reports:

- installation errors that would prevent the package from being installed
- quality warnings for common authoring mistakes

## Common warnings

Warnings can include:

- missing `description` or `license`
- for manifests treated as part of a package registry, a path that does not
  match the registry path for the manifest's package ID
- source-content issues such as:
  - for sources with `contents.type = "archive"`:
    - `glob` entries in `fonts`
    - `fonts` or `ignore` rules that match nothing
    - font-like files that match neither `fonts` nor `ignore`

## Examples

```console
foton manifest check <manifest-path>
```

Skip source-dependent checks when validating many manifests, for example in a registry:

```console
foton manifest check --no-source-checks <registry-root>\packages\**\manifest.toml
```

Validate a manifest as part of a package registry rooted at a known directory:

```console
foton manifest check --registry-root <registry-root> <manifest-path>
```

Treat warnings as errors:

```console
foton --warnings-as-errors manifest check <manifest-path>
```

## Notes

- This command is primarily intended for package authors.
- Because the command fetches sources by default, network access may be required.
- A manifest that parses successfully can still fail `manifest check` if the
  sources are invalid or the selected fonts do not install correctly.

## Related pages

- [Writing a Package Manifest](../../../guide/advanced/write-package-manifest.md)
- [Package Manifest Reference](../../package-manifest.md)
- [manifest command reference](README.md)
