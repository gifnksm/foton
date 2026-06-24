# Package Manifest Reference

A package manifest is a TOML document that defines a single package version.
It is used both for packages stored in registries and for local manifest files
installed with `foton install --manifest`.

## Format overview

A manifest uses kebab-case field names.
Unknown fields are rejected.

At the top level, a manifest contains package metadata and a non-empty
`sources` array.

## Example

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
]
```

## Validation and quality checks

Use `foton manifest check` to validate a manifest.
The command checks both installation errors and quality issues.
For example, it can report:

- missing recommended fields such as `display-name`, `description`, or
  `license`
- duplicate display names after normalization
- exact `path` entries in `files` without `title`
- `files` or `ignore` rules that match nothing
- `glob` entries in `files`
- font-like files in a source that match neither `files` nor `ignore`

## Package version format and ordering

In a package manifest, the `version` field uses `foton`'s package-version
format.
It identifies a single immutable package release and is also used to order
package versions when `foton` selects newer or older releases.

### Format

A package version follows this grammar:

```text
<package-version> = <numeric-part> ("." <numeric-part>)* [<suffix>]
<suffix> = "-" <alpha-identifier> ("-" <suffix-identifier>)*
<numeric-part> = <digit>+
<alpha-identifier> = <lowercase-letter>+
<suffix-identifier> = <alpha-identifier> | <digit>+
<digit> = "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"
<lowercase-letter> = "a" | ... | "z"
```

Additional rules:

- `<suffix>` may appear only after the final `<numeric-part>`
- `<suffix>` denotes a pre-release of the same `<package-version>` without the
  `<suffix>`
- `<suffix-identifier>` values are written as separate `-`-delimited tokens,
  so use `1.0.0-rc-10`, not `1.0.0-rc10`

Valid examples:

- `2407.24`
- `2024.05.11`
- `1.2.3`
- `1.0.0-rc-1`

Invalid examples:

- `1-rc.10`
- `1.0-beta.2`
- `1-rc10`
- `1-RC`

### Comparison rules

Versions are compared in this order:

1. Compare corresponding `<numeric-part>` values from left to right.
2. Compare each `<numeric-part>` by numeric value.
3. If two `<numeric-part>` values have the same numeric value, the one with
   fewer leading zeros is older.
4. If all shared `<numeric-part>` values are equal and one `<package-version>`
   has fewer `<numeric-part>` values, the shorter version is older.
5. A `<package-version>` with a `<suffix>` is a pre-release and is older than
   the same `<package-version>` without a `<suffix>`.
6. If both versions have `<suffix>` values, compare corresponding
   `<suffix-identifier>` values from left to right.
7. Numeric `<suffix-identifier>` values are compared by numeric value and are
   older than alphabetic `<suffix-identifier>` values.
8. If two numeric `<suffix-identifier>` values have the same numeric value,
   the one with fewer leading zeros is older.
9. If all shared `<suffix-identifier>` values are equal and one `<suffix>` has
   fewer identifiers, the shorter suffix is older.

### Comparison examples

- Numeric parts are compared from left to right:
  - `1.2.0 < 1.10.0`
  - `1.2 < 1.2.0`
  - `1 < 1.0-rc < 1.0`
- Leading zeros affect ordering when the numeric values are otherwise equal:
  - `5 < 05 < 005`
  - `2024.5.11 < 2024.05.11`
- A suffix makes a version older than the same version without a suffix:
  - `1.0.0-rc < 1.0.0`
  - `2024.05.11-beta-2 < 2024.05.11`
- Suffix identifiers are compared from left to right:
  - `1.0.0-alpha < 1.0.0-beta`
  - `1.0.0-rc-2 < 1.0.0-rc-10`
  - `1.0.0-rc-1 < 1.0.0-rc-a`
  - `1.0.0-rc < 1.0.0-rc-1`

## Top-level fields

Field headings indicate whether a field is required or optional.
Fields marked recommended are optional, but strongly recommended because they
help users discover and understand the fonts provided by the package.
Metadata fields such as `display-name`, `description`, `aliases`, `homepage`,
`repository`, and `license` describe the fonts provided by the package.
Packaging fields such as `name`, `version`, and `sources` describe how `foton`
identifies and installs the package.

### `name` (required)

The canonical package name used in commands such as
`foton install <package-name>`.
This is the stable identifier for the package.

- **Type**: package name string
- **Constraints**: must start with a lowercase ASCII letter and contain only
  lowercase ASCII letters, digits, or `-`
- **Example**:

  ```toml
  name = "example-font"
  ```

### `display-name` (optional, recommended)

A human-friendly primary name for the font family, collection, or bundle
provided by the package.
Use this for the primary label shown to users in search results and other
output.

- **Type**: string
- **Constraints**: must be non-empty and must not have leading or trailing
  whitespace
- **Recommended because**: it gives users a clear primary name for the fonts
  provided by the package
- **Example**:

  ```toml
  display-name = "Example Font"
  ```

### `version` (required)

The package version.
Use a version string that identifies an immutable package release.

- **Type**: package version string
- **Constraints**: must follow the package-version format described above
- **Example**:

  ```toml
  version = "1.2.3"
  ```

### `description` (optional, recommended)

A short description of the font family, collection, or bundle provided by the
package.

- **Type**: string
- **Constraints**: must be non-empty and must not have leading or trailing
  whitespace
- **Recommended because**: it appears in search results and package details
- **Example**:

  ```toml
  description = "Example font family for UI and coding"
  ```

### `aliases` (optional, recommended)

Alternative names and spellings for the font family, collection, or bundle used
for search.
Use this for family or collection names, abbreviations, and alternate
spellings.

- **Type**: array of strings
- **Constraints**: each entry must be non-empty and must not have leading or
  trailing whitespace
- **Recommended because**: it helps users find the package by alternate names
  and spellings
- **Example**:

  ```toml
  aliases = ["Example Font UI", "Example Font Console"]
  ```

### `homepage` (optional, recommended)

The upstream homepage for the font project or distribution represented by the
package.

- **Type**: URL string
- **Constraints**: the URL scheme must be `http` or `https`; omit this field if
  there is no distinct upstream homepage
- **Recommended because**: it gives users a homepage for more information about
  the fonts provided by the package
- **Note**: do not duplicate `repository` here when both would be the same URL
- **Example**:

  ```toml
  homepage = "https://example.com/example-font"
  ```

### `repository` (optional, recommended)

The upstream source repository for the font project represented by the package.

- **Type**: URL string
- **Constraints**: the URL scheme must be `http` or `https`; omit this field if
  there is no suitable public upstream repository
- **Recommended because**: it gives users a source repository for the upstream
  font project
- **Example**:

  ```toml
  repository = "https://github.com/example/example-font"
  ```

### `license` (optional, recommended)

The SPDX license expression for the upstream font files included in the
package.

- **Type**: SPDX expression string
- **Constraints**: must be a valid SPDX expression
- **Recommended because**: it tells users the licensing terms of the font files
  included in the package
- **Example**:

  ```toml
  license = "OFL-1.1"
  ```

### `sources` (required)

A non-empty array of source objects.
Each source describes one downloadable archive or file from which `foton` can
install fonts.

- **Type**: non-empty array of source objects
- **Example**:

  ```toml
  [[sources]]
  url = "https://example.com/downloads/example-font-1.2.3.zip"
  hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
  ```

## Source fields

Field headings indicate whether a field is required or optional.
Each entry in `sources` supports the following fields.

### `sources[].url` (required)

The downloadable archive or file that contains the package contents.

- **Type**: URL string
- **Constraints**: the URL scheme must be `http` or `https`
- **Example**:

  ```toml
  [[sources]]
  url = "https://example.com/downloads/example-font-1.2.3.zip"
  ```

### `sources[].hash` (required)

The expected digest used to verify source integrity.

- **Type**: digest string
- **Constraints**: must include an algorithm prefix such as `sha256:`;
  currently `sha256` is supported
- **Example**:

  ```toml
  [[sources]]
  hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
  ```

### `sources[].files` (optional, recommended)

Rules that select font files from the downloaded source.
Each entry is either:

- a string, which is shorthand for an exact `path`
- an object that specifies exactly one of `path` or `glob`

If an entry uses `path`, it may also specify `title`.
If an entry uses `glob`, it must not specify `title`.
If omitted, `foton` uses the default font-file glob rules.

- **Type**: non-empty array of file rules
- **Constraints**:
  - each exact `path` must be non-empty
  - each `glob` must be valid and non-empty
  - `title` may be specified only together with `path`
  - when present, `title` must be non-empty and must not have leading or
    trailing whitespace
- **Default**: `{ glob = "**/*.ttf" }`, `{ glob = "**/*.otf" }`,
  `{ glob = "**/*.ttc" }`, and `{ glob = "**/*.otc" }`
- **Recommended because**: it makes the package contents explicit and reduces
  unintended matches from the source archive
- **Notes**:
  - `title` is the name `foton` uses for that font file when registering it on
    Windows; other applications can also refer to the font by that name
  - `title` is also used when matching search queries
  - if an exact `path` entry omits `title`, `foton` detects the name from the
    font file instead of taking it from the manifest
  - `glob` entries cannot declare `title`; for files matched by `glob`,
    `foton` detects each name from the font file
- **Examples**:

  ```toml
  [[sources]]
  files = [
    { path = "fonts/ExampleFont-Regular.ttf", title = "Example Font Regular" },
    { path = "fonts/ExampleFont-Bold.ttf", title = "Example Font Bold" },
    { glob = "fonts/ExampleFontUI-*.ttf" },
  ]
  ```

### `sources[].ignore` (optional)

Rules that exclude files even if they match `files`.
Each entry may be written as a string or `{ path = ... }` for an exact path, or
as `{ glob = ... }` for a glob rule.
If a path matches both `files` and `ignore`, `ignore` takes precedence.

- **Type**: array of ignore rules
- **Example**:

  ```toml
  [[sources]]
  ignore = [
    "fonts/Extra.ttf",
    { glob = "fonts/legacy/*.ttf" },
  ]
  ```

## Related pages

- [Writing a Package Manifest](../guide/advanced/write-package-manifest.md)
- [Package Registry Reference](package-registry.md)
- [manifest check command reference](commands/manifest/check.md)
