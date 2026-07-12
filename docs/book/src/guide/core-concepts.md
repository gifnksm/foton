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

Use `<package-name>` when you want to refer to a package by name and let the
command decide how to handle matching versions.
Use `<package-name>@<version>` when you want to select an exact version, or
when a command asks you to disambiguate.

## Package registries

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

## Activation state

An installed package can be either `active` or `inactive`.

`Installed` means the package files are stored under `foton`'s local package
storage and the package is recorded in the local package database.
`Active` means that package's fonts are registered in the Windows registry and
available for normal use by applications.
`Inactive` packages remain installed, but their fonts are not registered for
use until you activate them.

These states are separate so that you can keep packages installed without
keeping all of them active at the same time.
For example, you may want to activate only the fonts you currently need for
day-to-day use, while leaving other installed packages inactive until needed.

Keeping installation and activation separate also lets you keep multiple
installed versions of the same package and choose which version is active.

By default, `foton install` activates newly installed packages.
If you install with `--no-activate`, you can activate the package later with
`foton activate` and deactivate it again with `foton deactivate`.

## Where to go next

- For everyday usage, continue to [Basic Usage](basic/README.md)
- For detailed command behavior, see [Command Reference](../reference/commands/README.md)
