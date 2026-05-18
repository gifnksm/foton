# Core Concepts

This chapter introduces a few terms that appear throughout the rest of the
book.
You do not need to memorize them, but knowing them makes the command and guide
pages easier to follow.

## Packages

In `foton`, the main unit of installation is a package.
A package is a versioned definition of one or more fonts.
When you install, update, list, inspect, or uninstall something with `foton`,
you are working with packages.

## Package names and versions

Many commands accept either a package name or a package name with an exact
version.

- `<package-name>`
- `<package-name>@<version>`

Use `<package-name>` when you want `foton` to choose an appropriate version
for the command you are running.
Use `<package-name>@<version>` when you want to select an exact version.

## Registries

Packages are usually installed from package registries.
A package registry is a collection of package definitions that `foton` can
search and install from.

A package registry can be:

- a local directory on your machine
- a Git repository that `foton` fetches and caches locally

The default configuration includes the public `foton` package registry, which is backed by the
[`gifnksm/foton-registry`](https://github.com/gifnksm/foton-registry) repository.
You can also define your own package registries.
See [Setting Up Your Own Package Registry](advanced/setup-package-registry.md) for a practical guide.
For reference details, see [Package Registry Reference](../reference/package-registry.md) and
[Configuration File Reference](../reference/configuration.md).

## Manifest files

A manifest file is a TOML file that defines a package.
It contains package metadata and one or more downloadable sources from which
font files are installed.

Most users will work with packages from package registries.
Manifest files become important when you want to:

- install a package directly from a local manifest file
- validate a package definition with `foton manifest check`
- publish packages through your own package registry

See [Writing a Package Manifest](advanced/write-package-manifest.md) for a step-by-step guide
and [Package Manifest Reference](../reference/package-manifest.md) for a field-by-field reference.

## Where to go next

- For everyday usage, continue to [Basic Usage](basic/README.md)
- For detailed command behavior, see [Command Reference](../reference/commands/README.md)
