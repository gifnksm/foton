# Package Registry Reference

A package registry is a collection of package manifests that `foton` can search
and install from.
A registry may be stored in a local directory or in a Git repository.

## Directory layout

A registry is organized by package name and package version:

```text
<registry-root>/
  <package-name>/
    <version>/
      manifest.toml
```

Example:

```text
registry/
  example-font/
    1.2.3/
      manifest.toml
  another-font/
    2.0.0/
      manifest.toml
```

The package ID in the manifest must match the directory that contains it.
In other words, the manifest at
`example-font/1.2.3/manifest.toml` must describe `example-font@1.2.3`.

When `foton` scans the registry layout:

- under `<registry-root>`, only directories are considered package-name entries;
  hidden entries and other non-directory entries are ignored
- under each `<package-name>` directory, only directories are considered
  version entries; hidden entries and other non-directory entries are ignored
- within each `<version>` directory, `foton` reads `manifest.toml`; other files
  are ignored for package discovery

## Registry configuration

Registries are configured in your `config.toml` file under
`[registries.<registry-id>]`.
`<registry-id>` is a user-defined identifier such as `foton`, `local`, or
`team`.
Commands that support `--registry` use these registry IDs, not paths or URLs.

Each registry entry specifies a `source`.
`foton` currently supports these source formats:

- `local+<absolute-path>`
- `git+<url>`

A `local+...` source points directly at a directory on your machine.
A `git+...` source points at a Git repository URL and is fetched into a local
cache before use.

Example:

```toml
[registries.example]
source = "local+C:/path/to/my-registry"
enabled = true
```

```console
foton search --registry example <query>
```

For the exact configuration format, see
[Configuration File Reference](configuration.md).

## Related pages

- [Setting Up Your Own Package Registry](../guide/advanced/setup-package-registry.md)
- [Configuration File Reference](configuration.md)
- [search](commands/search.md)
- [install](commands/install.md)
