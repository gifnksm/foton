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
description = "Example font family for UI and coding"
aliases = [
  "Example Font UI",
  "Example Font Console",
]
homepage = "https://example.com/example-font"
repository = "https://github.com/example/example-font"
license = "OFL-1.1"

[[sources]]
url = "https://example.com/downloads/example-font-1.2.3.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
files = [
  { path = "example-font-1.2.3/ExampleFont-Regular.ttf", title = "Example Font Regular" },
  { path = "example-font-1.2.3/ExampleFont-Bold.ttf", title = "Example Font Bold" },
  { path = "example-font-1.2.3/ExampleFontUI-Regular.ttf", title = "Example Font UI Regular" },
  { path = "example-font-1.2.3/ExampleFontUI-Bold.ttf", title = "Example Font UI Bold" },
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

## Choosing a package version

Choose a `version` that identifies one specific immutable release and sorts in
release order for that package.

A practical way to choose package versions is:

- Use the upstream release version when it already fits `foton`'s
  package-version format.
- If there is no usable upstream version, use a calendar-based version such as
  `2024.05.11`.
- If you need to publish a pre-release, add a suffix to the final part. A
  suffix marks the version as a pre-release and sorts it before the
  corresponding version without a suffix, such as `1.4.0-rc-1 < 1.4.0` or
  `2024.05.11-beta-2 < 2024.05.11`.
- If the upstream version does not fit `foton`'s package-version syntax,
  rewrite it in a form that still preserves the upstream release ordering and
  whether the release is stable or pre-release. For example, rewrite `v1.4.0`
  as `1.4.0`, and rewrite `1.4.0-rc10` as `1.4.0-rc-10`.
- Keep the notation consistent within the same package to avoid non-intuitive
  ordering results:
  - Do not mix forms such as `2024.5.11` and `2024.05.11`, because they are
    different versions.
  - Keep the same number of numeric parts within a package. For example,
    prefer a consistent series such as `1.2.0`, `1.3.0`, and `1.4.0-rc-1`
    over mixing forms such as `1.2`, `1.3.0`, and `1.4-rc-1`.

For the exact package-version syntax and ordering rules, see
[Package Manifest Reference](../../reference/package-manifest.md).

## Recommended fields

These fields are optional, but they are strongly recommended because they help
users discover and understand the fonts provided by the package.
See [Package Manifest Reference](../../reference/package-manifest.md) for the
complete field definitions and constraints.

These fields are recommended:

- `display-name`
- `description`
- `license`
- `aliases`
- `homepage`
- `repository`

These metadata fields describe the fonts provided by the package, not the
package definition itself. In particular, `homepage`, `repository`, and
`license` should refer to the upstream font project or the upstream font files
included in the package.

If there is no suitable upstream homepage or repository, omit that field. Do
not repeat `repository` in `homepage` just to fill both fields.

`foton manifest check` warns if `display-name`, `description`, or `license` is
missing.

## Choosing files from a source

Each `sources[]` entry can define `sources[].files` and `sources[].ignore`
rules.
Use them to control which files from the downloaded archive or file are treated
as installable fonts.
See [Package Manifest Reference](../../reference/package-manifest.md) for the exact
behavior of `sources`, `sources[].files`, and `sources[].ignore`.

If you omit `sources[].files`, `foton` uses these default glob rules:

- `**/*.ttf`
- `**/*.otf`
- `**/*.ttc`
- `**/*.otc`

Prefer `sources[].files` entries that list each font file path explicitly.
When you know the font title in advance, prefer `{ path = ..., title = ... }`
entries.
The `title` is the name `foton` uses when registering that font file on
Windows. Other applications can also see and refer to the font by that
name. If you specify it, `foton` uses that name instead of detecting one
from the font file.
Avoid `glob` rules when possible.
This makes it clear from the manifest exactly which files belong to the
package, and it reduces the chance of unintentionally picking up extra or
unexpected files from the source archive.

If the source archive contains other font-like files such as `*.ttf`, `*.otf`,
`*.ttc`, or `*.otc` that you do not want to install, prefer listing those paths in
`sources[].ignore` explicitly.
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
- duplicated display names
- exact `path` entries in `files` without `title`
- `glob` entries in `files`
- `files` or `ignore` rules that match nothing
- font-like files that match neither `files` nor `ignore`

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
- [manifest check command reference](../../reference/commands/manifest/check.md)
