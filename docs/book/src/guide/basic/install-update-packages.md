# Installing and Updating Packages

This chapter covers the commands you will use most often after discovering a
package: `install` and `update`.

In the examples below, replace placeholders such as `<package-name>`,
`<version>`, `<registry-id>`, and `<manifest-path>` with real values.

## Install packages from package registries

Install a package by name:

```console
foton install <package-name>
```

Install multiple packages at once:

```console
foton install <package-name-1> <package-name-2>
```

Install a specific version:

```console
foton install <package-name>@<version>
```

By default, `foton` resolves packages from the package registries enabled in
your configuration.
To resolve packages only from specific package registries, pass `--registry`.

```console
foton install --registry <registry-id-1>,<registry-id-2> <package-name>
```

## Install without activating immediately

By default, `install` activates the selected packages after they are installed.
If you only want to download and record the package first, pass
`--no-activate`.

```console
foton install --no-activate <package-name>
```

You can activate the package later with `foton activate`.

## Install packages from manifest files

`install` can also install packages defined in local manifest files.
This is mainly useful when authoring or testing packages.

```console
foton install --manifest <manifest-path>
```

You can specify `--manifest` multiple times.

For more details, see
[Writing a Package Manifest](../advanced/write-package-manifest.md).

## Update installed packages

Update the latest installed version of each package that has a newer version
available:

```console
foton update
```

Update only selected packages:

```console
foton update <package-name-1> <package-name-2>
```

You can also select an exact installed version first:

```console
foton update <package-name>@<version>
```

Without an exact version, `update` selects the latest installed version for
that package name.
If you want to control exactly which installed version is updated, specify an
exact version.
Updating a package installs the newer version without automatically removing
older installed versions.

If you want to control which package registries are used to find updates, pass
`--registry`.

```console
foton update --registry <registry-id-1>,<registry-id-2> <package-name>
```

## Confirmation prompts

Commands that change installed packages ask for confirmation before applying
changes.
If you want to skip the prompt, pass the global `--no-confirm` option.

```console
foton --no-confirm install <package-name>
```

```console
foton --no-confirm update
```

## Notes

- If an install request does not require any changes, `foton` reports that the
  package is already installed.
- If an update request does not require any changes, `foton` reports that the
  selected packages are already up to date.
- If an install or update does not complete cleanly, use `foton repair` to
  clean up any packages left behind.
- See the command reference for details such as pre-release handling and more
  specific resolution rules.

## Related pages

- [Discovering Packages](discover-packages.md)
- [Managing Installed Packages](manage-installed-packages.md)
- [install command reference](../../reference/commands/install.md)
- [update command reference](../../reference/commands/update.md)
- [repair command reference](../../reference/commands/repair.md)
