# manifest

Work with package manifest files.

## Usage

```text
foton manifest <COMMAND>
```

## Commands

- [`check`](check.md): validate manifest files for installation errors and
  quality warnings

## Typical usage

The `manifest` command group is mainly intended for package authors.
A common workflow is:

1. Write a manifest file
2. Run `foton manifest check <MANIFEST>`
3. Install the manifest locally with `foton install --manifest <MANIFEST>`
4. Add the manifest to a package registry

## Related pages

- [Writing a Package Manifest](../../../guide/advanced/write-package-manifest.md)
- [Package Manifest Reference](../../package-manifest.md)
