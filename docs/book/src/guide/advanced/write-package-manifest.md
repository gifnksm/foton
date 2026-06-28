# Writing a Package Manifest

A package manifest is a TOML file that defines a font package.
It tells `foton` what the package is called, where its downloadable sources are,
and which fonts from those sources should be installed.

## Typical workflow

A practical workflow for authoring a package is:

1. Write a manifest file
2. Run `foton manifest check` on it
3. Install it locally with `foton install --manifest`
4. Add it to a package registry if you want to publish it

## Example manifest

```toml
name = "example-font"
version = "1.2.3"
description = "Example font family for UI and coding"
homepage = "https://example.com/example-font"
repository = "https://github.com/example/example-font"
license = "OFL-1.1"

[[sources]]
url = "https://example.com/downloads/example-font-1.2.3.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

[sources.contents]
type = "archive"
fonts = [
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

For the exact fields inside each source entry, see
[Package Manifest Reference](../../reference/package-manifest.md).

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

These fields are optional, but strongly recommended because they help users
understand the package and the fonts it provides. In particular,
`description` appears in search results and is used by `foton search`.
See [Package Manifest Reference](../../reference/package-manifest.md) for the
complete field definitions and constraints.

These fields are recommended:

- `description`
- `license`
- `homepage`
- `repository`

These metadata fields describe the package and the fonts it provides, not the
manifest structure itself. In particular, `homepage`, `repository`, and
`license` should refer to the upstream font project or the upstream font files
included in the package.

If there is no suitable upstream homepage or repository, omit that field. Do
not repeat `repository` in `homepage` just to fill both fields.

`foton manifest check` warns if `description` or `license` is missing.

## Choosing fonts from a source

Each `sources[]` entry must define a `[sources.contents]` table.
That table tells `foton` what kind of downloaded source it is working with and,
for archive sources, which font files inside that source should be installed.

If `contents.type = "archive"`, the downloaded source is an archive that
contains one or more font files.
If `contents.type = "font-file"`, the downloaded source itself is one font
file.
See [Package Manifest Reference](../../reference/package-manifest.md) for the exact
field definitions and constraints.

### Archive sources (`contents.type = "archive"`)

For an archive source, the `fonts` field under `[sources.contents]` selects
which archive entries should be installed as fonts.
If you omit `fonts`, `foton` uses these default glob rules:

- `**/*.ttf`
- `**/*.otf`
- `**/*.ttc`
- `**/*.otc`

Prefer `fonts` entries that list each font file path explicitly.
If the archive entry's file name is unsuitable, you can also specify
`file-name` on a `path` entry written as `{ path = ... }` to tell `foton`
what file name to store locally.
Avoid `glob` rules when possible.
This makes it clear from the manifest exactly which files belong to the
package, and it reduces the chance of unintentionally picking up extra or
unexpected files from the source archive.

If the source archive contains other font-like files such as `*.ttf`, `*.otf`,
`*.ttc`, or `*.otc` that you do not want to install, prefer listing those paths in
`ignore` explicitly.
That makes the omission visible in the manifest and shows that the files were
left out intentionally.

### Direct font sources (`contents.type = "font-file"`)

For a source with `contents.type = "font-file"`, the downloaded source itself
is installed as one font file.
You can set `file-name` to override the local file name when the URL path is
not suitable.

```toml
[[sources]]
url = "https://example.com/download?id=123"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

[sources.contents]
type = "font-file"
file-name = "Example-Regular.ttf"
```

## Validate the manifest

Run:

```console
foton manifest check <manifest-path>
```

`manifest check` does more than syntax validation.
It reads the manifest, stages the package, downloads the sources, and verifies
that installation would succeed.
It can also warn about issues such as:

- missing `description` or `license`

It can also warn specifically about source contents, for example:

- for sources with `contents.type = "archive"`:
  - `glob` entries in `fonts`
  - `fonts` or `ignore` rules that match nothing
  - font-like files that match neither `fonts` nor `ignore`

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
