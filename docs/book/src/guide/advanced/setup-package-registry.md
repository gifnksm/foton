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

A custom package registry can be managed in two common ways:

- as a local directory on your machine
- as a Git repository that stores the registry root and its manifests

A local directory is convenient for local testing or personal use.
A Git-backed registry is useful when you want version history, code review, or
sharing across multiple machines or users.

The default configuration already includes the public `foton` package registry.
Custom registries are added through your `config.toml` file.

## Registry layout

A registry stores package manifests in this directory layout:

```text
<registry-root>/
  .foton-registry-root
  packages/
    <package-name>/
      <version>/
        manifest.toml
```

Example:

```text
my-registry/
  .foton-registry-root
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
The `.foton-registry-root` marker file is recommended. It is not required for
package discovery, but it makes the registry root explicit and helps
`foton manifest check` detect the registry root automatically.

## Setup workflow

A practical workflow is:

1. Create the registry root directory and add `.foton-registry-root`
2. Place each manifest at `packages/<package-name>/<version>/manifest.toml`
3. Validate the manifests in place
4. Add the registry to your `config.toml` file
5. Use that registry from `foton`

The sections below cover each part of that workflow.

## Create the registry root

Create the registry root directory and add the `.foton-registry-root` marker
file.
This makes the registry root explicit and lets `foton manifest check` detect it
automatically when validating manifests by file path.

If you want to manage the registry through Git, make the registry root a Git
repository, for example by running `git init` there or by placing it inside an
existing repository.

## Add package manifests

Place each manifest at `packages/<package-name>/<version>/manifest.toml` in the
registry.
If you need help writing a manifest, see
[Writing a Package Manifest](write-package-manifest.md).

## Validate package manifests

When the manifest is already stored under a registry root that contains
`.foton-registry-root`, `manifest check` can detect that registry root
automatically.
In the normal case, you can validate a manifest in place with:

```console
foton manifest check <manifest-path>
```

When validating many manifests in a registry at once, it can be useful to skip
source-dependent checks first:

```console
foton manifest check --no-source-checks <registry-root>\packages\**\manifest.toml
```

## Add the registry to your config

Once the manifests are in place, add the registry to your `config.toml` file.
You can configure it either as a local directory or as a Git-backed registry.

### Local registry source

Use `local+<absolute-path>` for a registry stored in a local directory.
The path must be absolute.

```toml
[registries.example]
source = "local+C:/path/to/my-registry"
enabled = true
```

### Git registry source

Use `git+<url>` for a registry stored in a Git repository.

```toml
[registries.example]
source = "git+https://example.com/fonts/example-registry.git"
enabled = true
```

`foton` caches Git registries locally and updates the cached repository when it
fetches registry contents.

### Enable or disable a registry

Each registry entry has an `enabled` flag.
If omitted, it defaults to `true`.
Set it to `false` when you want to keep the registry definition in your
`config.toml` file without using it by default.

```toml
[registries.example]
source = "local+C:/path/to/experimental-registry"
enabled = false
```

## Use the registry from `foton`

Search or install from that package registry by ID:

```console
foton search --registry example <query>
```

```console
foton install --registry example <package-name>
```

You can also opt in to a disabled package registry explicitly with
`--registry <registry-id>`.
If multiple package registries contain the same package name, use
`--registry` to choose which registry to use.

## Related pages

- [Package Registry Reference](../../reference/package-registry.md)
- [Configuration File Reference](../../reference/configuration.md)
