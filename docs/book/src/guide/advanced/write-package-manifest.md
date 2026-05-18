# Writing a Package Manifest

A package manifest is a TOML file that defines a font package.
It tells `foton` what the package is called, where its downloadable sources are,
and which files from those sources should be installed as fonts.

## Typical workflow

A practical workflow for authoring a package is:

1. Write a manifest file
2. Run `foton manifest check` on it
3. Install it locally with `foton install --manifest`
4. Add it to a package registry if you want to publish it

## Example manifest

```toml
name = "example-font"
display-name = "Example Font"
version = "1.2.3"
description = "Example package description"
aliases = [
  "Example Font UI",
  "Example Font Console",
]
faces = [
  "Example Font Regular",
  "Example Font Bold",
  "Example Font UI Regular",
  "Example Font UI Bold",
]
homepage = "https://example.com/example-font"
repository = "https://example.com/example-font/repository"
license = "MIT"

[[sources]]
url = "https://example.com/downloads/example-font-1.2.3.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
include = [
  "example-font-1.2.3/ExampleFont-Regular.ttf",
  "example-font-1.2.3/ExampleFont-Bold.ttf",
  "example-font-1.2.3/ExampleFontUI-Regular.ttf",
  "example-font-1.2.3/ExampleFontUI-Bold.ttf",
]
```

## Required fields

This section is a quick checklist, not a complete field reference.
See [Package Manifest Reference](../../reference/package-manifest.md) for a detailed description of every field.

At minimum, a manifest must define:

- `name`
- `version`
- `sources`

Each `sources` entry must define:

- `url`
- `hash`

## Recommended fields

These fields are optional, but they are strongly recommended because they help
users discover and understand the package.
See [Package Manifest Reference](../../reference/package-manifest.md) for the complete
field definitions and constraints.

These fields are recommended:

- `display-name`
- `description`
- `license`
- `aliases`
- `faces`
- `homepage`
- `repository`

`foton manifest check` warns if `display-name`, `description`, or `license` is
missing.

## Choosing files from a source

Each `sources[]` entry can define `sources[].include` and `sources[].exclude`
patterns.
Use them to control which files from the downloaded archive or file are treated
as installable fonts.
See [Package Manifest Reference](../../reference/package-manifest.md) for the exact
behavior of `sources`, `sources[].include`, and `sources[].exclude`.

If you omit `sources[].include`, `foton` uses these default patterns:

- `**/*.ttf`
- `**/*.otf`
- `**/*.ttc`

Prefer `sources[].include` entries that list each font file path explicitly.
Avoid wildcard patterns when possible.
This makes it clear from the manifest exactly which files belong to the
package, and it reduces the chance of unintentionally picking up extra or
unexpected files from the source archive.

If the source archive contains other font-like files such as `*.ttf`, `*.otf`,
or `*.ttc` that you do not want to install, prefer listing those paths in
`sources[].exclude` explicitly.
That makes the omission visible in the manifest and shows that the files were
left out intentionally.

## Validate the manifest

Run:

```console
foton manifest check <manifest-path>
```

`manifest check` does more than syntax validation.
It reads the manifest, stages the package, downloads the sources, and verifies
that installation would succeed.
It can also warn about issues such as:

- missing `display-name`, `description`, or `license`
- duplicated display names or face names
- wildcard `include` patterns that are broader than necessary
- `include` or `exclude` patterns that match nothing
- font-like files that match neither `include` nor `exclude`

If you want warnings to fail the command, use the global
`--warnings-as-errors` option.

```console
foton --warnings-as-errors manifest check <manifest-path>
```

## Test the manifest locally

You can install a package directly from a local manifest file:

```console
foton install --manifest <manifest-path>
```

This is useful before publishing the manifest in a registry.
It lets you test the actual install workflow with the same manifest content.

## Publish through a registry

Once a manifest works locally, place it in a package registry so it can be
resolved by package name.
See [Setting Up Your Own Package Registry](setup-package-registry.md).

## Related pages

- [Package Manifest Reference](../../reference/package-manifest.md)
- [manifest check](../../reference/commands/manifest/check.md)
