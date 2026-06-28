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
]
```

## Validation and quality checks

Use `foton manifest check` to validate a manifest.
The command checks both installation errors and quality issues.
For example, it can report:

- missing recommended fields such as `description` or `license`
- source-content issues such as:
  - for sources with `contents.type = "archive"`:
    - `glob` entries in `fonts`
    - `fonts` or `ignore` rules that match nothing
    - font-like files that match neither `fonts` nor `ignore`

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
help users understand the package and the fonts it provides. In particular,
`description` appears in search results and is used by `foton search`.
Metadata fields such as `description`, `homepage`, `repository`, and `license`
describe the package and the fonts it provides.
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
- **Recommended because**: it appears in search results and can help users
  find the package
- **Example**:

  ```toml
  description = "Example font family for UI and coding"
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
Each source describes one downloadable font file or archive from which `foton`
can install fonts.

- **Type**: non-empty array of source objects
- **Example**:

  ```toml
  [[sources]]
  url = "https://example.com/downloads/example-font-1.2.3.zip"
  hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

  [sources.contents]
  type = "archive"
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

### `sources[].contents` (required)

A table that describes what the downloaded source contains.
Use `contents.type = "font-file"` when the downloaded source is itself one
font file.
Use `contents.type = "archive"` when the downloaded source is an archive that
contains one or more font files.

- **Type**: contents table
- **Example**:

  ```toml
  [[sources]]
  url = "https://example.com/downloads/example-font-1.2.3.zip"
  hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

  [sources.contents]
  type = "archive"
  ```

### `sources[].contents.type` (required)

The kind of downloaded source.

- **Type**: string
- **Allowed values**:
  - `"font-file"` — the downloaded source is one font file
  - `"archive"` — the downloaded source is an archive that contains font files
- **Example**:

  ```toml
  [sources.contents]
  type = "font-file"
  ```

### `sources[].contents.file-name` (optional, for sources with `contents.type = "font-file"`)

The file name that `foton` should use for a source with
`contents.type = "font-file"`.
When omitted, `foton` derives the file name from the URL path.

- **Type**: file name string
- **Constraints**: must be a valid plain file name
- **Example**:

  ```toml
  [[sources]]
  url = "https://example.com/download?id=123"
  hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

  [sources.contents]
  type = "font-file"
  file-name = "Example-Regular.ttf"
  ```

### `sources[].contents.fonts` (optional, recommended for sources with `contents.type = "archive"`)

Rules that select font files from a source with
`contents.type = "archive"`.
Each entry is either:

- a `path` entry, written either as a string or as `{ path = ... }`
- a `glob` entry, written as `{ glob = ... }`

A string `path` entry is shorthand for `{ path = "..." }`.
Only `path` entries written as `{ path = ... }` may also specify
`file-name`.
`glob` entries must not specify `file-name`.
If omitted, `foton` uses the default font-file glob rules.

- **Type**: non-empty array of font rules
- **Constraints**:
  - each `path` entry must specify a non-empty path
  - each `glob` entry must specify a valid, non-empty glob
  - `file-name` may be specified only on `path` entries written as
    `{ path = ... }`
  - when present, `file-name` must be a valid plain file name
- **Default**: `{ glob = "**/*.ttf" }`, `{ glob = "**/*.otf" }`,
  `{ glob = "**/*.ttc" }`, and `{ glob = "**/*.otc" }`
- **Recommended because**: it makes the package contents explicit and reduces
  unintended matches from the source archive
- **Notes**:
  - if a `path` entry written as `{ path = ... }` specifies `file-name`,
    `foton` stores the extracted font file using that file name instead of
    the archive entry's original file name
  - if a `path` entry written as `{ path = ... }` does not specify
    `file-name`, `foton` keeps the archive entry's original file name
  - string `path` entries and `glob` entries keep each archive entry's
    original file name
- **Examples**:

  ```toml
  [[sources]]
  url = "https://example.com/downloads/example-font-1.2.3.zip"
  hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

  [sources.contents]
  type = "archive"
  fonts = [
    "fonts/ExampleFont-Regular.ttf",
    { path = "fonts/ExampleFont-Bold.ttf", file-name = "Example-Bold.ttf" },
    { glob = "fonts/ExampleFontUI-*.ttf" },
  ]
  ```

### `sources[].contents.ignore` (optional, for sources with `contents.type = "archive"`)

Rules that exclude archive entries from a source with
`contents.type = "archive"` even if they match `fonts`.
Each entry may be written as:

- a `path` entry, written either as a string or as `{ path = ... }`
- a `glob` entry, written as `{ glob = ... }`

If a path matches both `fonts` and `ignore`, `ignore` takes precedence.

- **Type**: array of ignore rules
- **Example**:

  ```toml
  [[sources]]
  url = "https://example.com/downloads/example-font-1.2.3.zip"
  hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

  [sources.contents]
  type = "archive"
  ignore = [
    "fonts/Extra.ttf",
    { glob = "fonts/legacy/*.ttf" },
  ]
  ```

## Related pages

- [Writing a Package Manifest](../guide/advanced/write-package-manifest.md)
- [Package Registry Reference](package-registry.md)
- [manifest check command reference](commands/manifest/check.md)
