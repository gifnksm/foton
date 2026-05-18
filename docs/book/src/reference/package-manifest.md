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
]
```

## Validation and quality checks

Use `foton manifest check` to validate a manifest.
The command checks both installation errors and quality issues.
For example, it can report:

- missing recommended fields such as `display-name`, `description`, or
  `license`
- duplicate display names or face names after normalization
- `include` or `exclude` patterns that match nothing
- wildcard `include` patterns that are broader than necessary
- font-like files in a source that are neither included nor excluded

## Top-level fields

Field headings indicate whether a field is required or optional.
Fields marked recommended are optional, but strongly recommended because they
help users discover and understand the package.

### `name` (required)

The canonical package name used in commands such as
`foton install <package-name>`.
This is the stable identifier for the package.

- **Type**: package name string
- **Constraints**: must start with an ASCII letter and contain only ASCII
  letters, digits, `-`, or `_`
- **Example**:

  ```toml
  name = "example-font"
  ```

### `display-name` (optional, recommended)

A user-facing primary name for the package.
Use this for the main label you want users to see.

- **Type**: string
- **Constraints**: must be non-empty and must not have leading or trailing
  whitespace
- **Recommended because**: it gives users a clear primary name for the package
- **Example**:

  ```toml
  display-name = "Example Font"
  ```

### `version` (required)

The package version.
Use a version string that identifies an immutable package release.

- **Type**: semantic version string
- **Constraints**: must be a valid SemVer version
- **Example**:

  ```toml
  version = "1.2.3"
  ```

### `description` (optional, recommended)

A short description shown in search results and package details.

- **Type**: string
- **Constraints**: must be non-empty and must not have leading or trailing
  whitespace
- **Recommended because**: it appears in search results and package details
- **Example**:

  ```toml
  description = "Example package description"
  ```

### `aliases` (optional, recommended)

Alternative package-level names and spellings used for search.
Use this for package or family names, abbreviations, and alternate spellings.

- **Type**: array of strings
- **Constraints**: each entry must be non-empty and must not have leading or
  trailing whitespace
- **Recommended because**: it helps users find the package by alternate names
  and spellings
- **Example**:

  ```toml
  aliases = ["Example Font UI", "Example Font Console"]
  ```

### `faces` (optional, recommended)

Human-friendly names for the individual font faces included in the package.
Use this for entries such as Regular, Bold, or other specific face names.

- **Type**: array of strings
- **Constraints**: each entry must be non-empty and must not have leading or
  trailing whitespace
- **Recommended because**: it helps users find the package by included face
  names
- **Example**:

  ```toml
  faces = ["Example Font Regular", "Example Font Bold"]
  ```

### `homepage` (optional, recommended)

A homepage for the project or package.

- **Type**: URL string
- **Constraints**: the URL scheme must be `http` or `https`
- **Recommended because**: it gives users a homepage for more information about
  the package
- **Example**:

  ```toml
  homepage = "https://example.com/example-font"
  ```

### `repository` (optional, recommended)

A source repository for the package definition or the upstream font project.

- **Type**: URL string
- **Constraints**: the URL scheme must be `http` or `https`
- **Recommended because**: it gives users a source repository for the package
  definition or upstream project
- **Example**:

  ```toml
  repository = "https://example.com/example-font/repository"
  ```

### `license` (optional, recommended)

The package license in SPDX expression form.

- **Type**: SPDX expression string
- **Constraints**: must be a valid SPDX expression
- **Recommended because**: it tells users the package licensing terms
- **Example**:

  ```toml
  license = "MIT"
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

### `sources[].include` (optional, recommended)

Glob patterns that select font files from the downloaded source.
If omitted, `foton` uses the default font-file patterns.

- **Type**: array of glob patterns
- **Constraints**: must be non-empty if present
- **Default**: `**/*.ttf`, `**/*.otf`, and `**/*.ttc`
- **Recommended because**: it makes the package contents explicit and reduces
  unintended matches from the source archive
- **Example**:

  ```toml
  [[sources]]
  include = [
    "fonts/ExampleFont-Regular.ttf",
    "fonts/ExampleFont-Bold.ttf",
  ]
  ```

### `sources[].exclude` (optional)

Glob patterns that exclude files even if they match `include`.
If a path matches both `include` and `exclude`, `exclude` takes precedence.

- **Type**: array of glob patterns
- **Example**:

  ```toml
  [[sources]]
  exclude = [
    "fonts/Extra.ttf",
  ]
  ```

## Related pages

- [Writing a Package Manifest](../guide/advanced/write-package-manifest.md)
- [Package Registry Reference](package-registry.md)
- [manifest check](commands/manifest/check.md)
