# Setting Up Your Own Package Registry

A package registry is a collection of package manifests that `foton` can search
and install from.
You can use your own package registry to distribute internal packages, test
packages before publishing them elsewhere, or maintain a curated set of fonts.

This chapter focuses on the setup workflow.
For the exact registry layout and configuration format, see
[Package Registry Reference](../../reference/package-registry.md) and
[Configuration File Reference](../../reference/configuration.md).

## Registry types

`foton` supports two kinds of package registry sources:

- `local+<absolute-path>` for a registry stored in a local directory
- `git+<url>` for a registry stored in a Git repository

The default configuration already includes the public `foton` package registry.
Custom package registries are added through your `config.toml` file.

## Registry layout

A registry stores package manifests in this directory layout:

```text
<registry-root>/
  packages/
    <package-name>/
      <version>/
        manifest.toml
```

Example:

```text
my-registry/
  packages/
    example-font/
      1.2.3/
        manifest.toml
    another-font/
      2.0.0/
        manifest.toml
```

Each package version has its own `manifest.toml` file.
This layout lets a single registry contain multiple versions of the same
package.

## Add a local registry

A local registry path must be absolute.
Add an entry to your `config.toml` file:

```toml
[registries.example]
source = "local+C:/path/to/my-registry"
enabled = true
```

After that, you can search or install from that package registry by ID:

```console
foton search --registry example <query>
```

```console
foton install --registry example <package-name>
```

## Add a Git registry

To use a registry from Git, add a `git+` source to your `config.toml` file:

```toml
[registries.example]
source = "git+https://example.com/fonts/example-registry.git"
enabled = true
```

Then use it the same way:

```console
foton search --registry example <query>
```

```console
foton install --registry example <package-name>
```

`foton` caches Git registries locally and updates the cached repository when it
fetches registry contents.

## Enable or disable a registry

Each registry entry has an `enabled` flag.
If omitted, it defaults to `true`.
Set it to `false` when you want to keep the registry definition in your
`config.toml` file without using it by default.
You can still opt in to a disabled package registry explicitly with
`--registry <registry-id>`.
If all configured package registries are disabled, commands such as `search`,
`install`, and `update` fail unless you specify `--registry`.

```toml
[registries.example]
source = "local+C:/path/to/experimental-registry"
enabled = false
```

## Publish your own packages

A common workflow is:

1. Write and validate a manifest locally
2. Place the manifest at `packages/<package-name>/<version>/manifest.toml` in your
   registry
3. Add the registry to your `config.toml` file
4. Search or install from that package registry with `--registry <registry-id>`

If you enable multiple package registries that contain the same package name,
`install` and `update` may ask you to disambiguate by narrowing `--registry`.

## Related pages

- [Package Registry Reference](../../reference/package-registry.md)
- [Configuration File Reference](../../reference/configuration.md)
