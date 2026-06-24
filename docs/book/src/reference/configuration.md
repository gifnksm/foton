# Configuration File Reference

`foton` loads configuration from a `config.toml` file in its configuration
directory.
If the file does not exist, built-in defaults are used.

On Windows, the configuration file is stored at:

```text
%APPDATA%\io.github.gifnksm\foton\config\config.toml
```

## What configuration controls

The configuration file currently controls:

- install-time safety limits for downloaded package sources and the font files
  selected from them
- the set of configured package registries
- whether each configured registry is enabled by default

## Default behavior and merge rules

`config.toml` is optional.
If it is present, `foton` merges it onto built-in defaults.

This means:

- omitted sections and keys keep their default values
- unknown keys are rejected
- the default configuration already includes the public `foton` package registry

## Example configuration

```toml
[install]
max-source-size-bytes = 536870912
max-fonts-per-package-source = 1000
max-font-file-size-bytes = 52428800

[registries.foton]
source = "git+https://github.com/gifnksm/foton-registry.git"
enabled = true

[registries.local]
source = "local+C:/path/to/my-registry"
enabled = true

[registries.experimental]
source = "git+https://example.com/fonts/experimental-registry.git"
enabled = false
```

## Top-level sections

Field headings indicate whether a section or key is optional.
Defaults are listed in the metadata bullets for the relevant sections and keys.

### `install` (optional)

The `install` section defines safety limits used while processing package
sources.
All install limit values must be positive integers greater than zero.

- **Type**: table
- **Default**: built-in install limits are used if this section is omitted
- **Example**:

  ```toml
  [install]
  max-source-size-bytes = 536870912
  max-fonts-per-package-source = 1000
  max-font-file-size-bytes = 52428800
  ```

### `registries` (optional)

The `registries` table contains `registries.<registry-id>` entries keyed by
registry ID.
Each `registries.<registry-id>` entry configures one registry that `foton` can
search or install from.

- **Type**: table of registry entries
- **Default**: the built-in `registries.foton` entry is present even if this
  section is omitted
- **Example**:

  ```toml
  [registries.example]
  source = "local+C:/path/to/my-registry"
  enabled = true
  ```

## Install section fields

### `install.max-source-size-bytes` (optional)

The maximum allowed size, in bytes, of one downloaded package source.
This limit applies before `foton` interprets the source as a direct font file or
as an archive.

- **Type**: positive unsigned integer
- **Default**: `536870912` (512MiB)
- **Example**:

  ```toml
  [install]
  max-source-size-bytes = 536870912
  ```

### `install.max-fonts-per-package-source` (optional)

The maximum number of font files that `foton` may take from one package
source.
For sources with `contents.type = "archive"`, this limits how many archive
entries may become installed font files.
For sources with `contents.type = "font-file"`, the source itself counts as
one font file.

- **Type**: positive unsigned integer
- **Default**: `1000`
- **Example**:

  ```toml
  [install]
  max-fonts-per-package-source = 1000
  ```

### `install.max-font-file-size-bytes` (optional)

The maximum allowed size, in bytes, of one font file selected for
installation.
This limit applies both to sources with `contents.type = "font-file"` and to
individual font files selected from sources with `contents.type = "archive"`.

- **Type**: positive unsigned integer
- **Default**: `52428800` (50MiB)
- **Example**:

  ```toml
  [install]
  max-font-file-size-bytes = 52428800
  ```

## Registry entry fields

### `registries.<registry-id>` (optional)

A single registry entry.
`<registry-id>` is a user-defined registry ID.
Commands such as `foton search --registry ...` and `foton install --registry ...`
use these registry IDs.

- **Type**: table
- **Constraints**: the registry ID must start with a lowercase ASCII letter and
  contain only lowercase ASCII letters, digits, or `-`
- **Example**:

  ```toml
  [registries.example]
  source = "local+C:/path/to/my-registry"
  enabled = true
  ```

### `registries.<registry-id>.source` (required)

The source from which `foton` loads the registry.
Use `local+...` for a local directory or `git+...` for a Git-backed registry.

- **Type**: registry source string
- **Constraints**: must be either `local+<absolute-path>` or `git+<url>`
- **Example**:

  ```toml
  [registries.example]
  source = "local+C:/path/to/my-registry"
  ```

  ```toml
  [registries.example]
  source = "git+https://example.com/fonts/example-registry.git"
  ```

### `registries.<registry-id>.enabled` (optional)

Whether the registry is enabled by default.
Set this to `false` when you want to keep the registry configured but not use
it unless you opt in explicitly.

If you pass `--registry <REGISTRY_ID>`, `foton` can still use that package
registry even when `enabled = false`.
If all configured package registries are disabled and you do not pass
`--registry`, commands such as `search`, `install`, and `update` fail because
there are no enabled package registries to use.

- **Type**: boolean
- **Default**: `true`
- **Example**:

  ```toml
  [registries.example]
  enabled = false
  ```

## Related pages

- [Package Registry Reference](package-registry.md)
- [Setting Up Your Own Package Registry](../guide/advanced/setup-package-registry.md)
