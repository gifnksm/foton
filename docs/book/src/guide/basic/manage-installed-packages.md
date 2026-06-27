# Managing Installed Packages

This chapter covers the commands you use after packages have already been
installed: `list`, `info`, `activate`, `deactivate`, and `uninstall`.
It also explains how to recover from incomplete operations with `repair`.

In the examples below, replace placeholders such as `<package-name>` and
`<version>` with real values.

## List installed packages

Show installed packages:

```console
foton list
```

By default, `list` shows packages in the `installed` state together with each
package's activation state.

If you also want to see packages left by incomplete operations, pass
`--show-incomplete`.

```console
foton list --show-incomplete
```

With `--show-incomplete`, each entry includes its installation state, such as
`installed`, `incomplete-install`, or `incomplete-uninstall`. Installed
packages also include their activation state.

If you see such packages, inspect them with `foton info`, then clean them up
with `foton repair`.
Most of the time, you will work only with `installed` packages.

## Inspect a package in detail

Show detailed information about one or more packages recorded in the local package database:

```console
foton info <package-name>
```

```console
foton info <package-name>@<version>
```

`info` prints the package ID, recorded states, and package metadata for
matching packages recorded in the local package database.
For packages in the `installed` state, it also shows a summary of the
installed font families.
Use `--show-files` when you also want the fonts directory and installed font
files for packages in the `installed` state.
This can include packages left by incomplete operations.
Use this command when you want to confirm exactly what is recorded in the local
package database.

## Change whether an installed package is active

Installed packages can stay available in `foton`'s local package storage while
being either `active` or `inactive`.
Active packages have their fonts registered in the Windows registry and
available for normal use by applications.
Inactive packages remain installed, but their fonts are not registered for use
until you activate them.

This is most useful when you installed a package with `--no-activate`, when
you want to keep only the fonts you currently need active for day-to-day use,
or when you want to switch versions manually.

Activate one or more installed packages:

```console
foton activate <package-name>
```

```console
foton activate <package-name>@<version>
```

Only one version of a package name can be active at a time.
When you activate one version, `foton` registers that version's fonts for use
and deactivates any other active version of that package name automatically.
If a package name matches multiple installed versions, specify an exact version
so `foton` knows which one to activate.

Deactivate one or more installed packages:

```console
foton deactivate <package-name>
```

```console
foton deactivate <package-name>@<version>
```

If the selected package is already inactive, `foton` reports that there is
nothing to do.
As with `activate`, specify an exact version when multiple installed versions
share the same package name.

Like other commands that change installed packages, `activate` and
`deactivate` ask for confirmation before applying changes.
Use the global `--no-confirm` option if you want to skip the prompt.

## Recover from incomplete operations

If `list --show-incomplete` shows packages left by incomplete operations, use
`repair` to clean them up:

```console
foton repair
```

You can also target a specific package:

```console
foton repair <package-name>
```

```console
foton repair <package-name>@<version>
```

`repair` cleans up those packages. It does not resume an interrupted install
or update.

## Remove a package

Uninstall one or more packages:

```console
foton uninstall <package-name>
```

```console
foton uninstall <package-name-1> <package-name-2>
```

Like `install` and `update`, `uninstall` asks for confirmation before applying
changes.
If an uninstall does not complete cleanly, use `foton repair` to clean up any
packages it leaves behind.
If you want to skip the prompt, pass the global `--no-confirm` option.

```console
foton --no-confirm uninstall <package-name>
```

## Typical workflow

A common workflow is:

1. Run `foton list` to see what is installed
2. Run `foton info <package-name>` to inspect a package in detail
3. Run `foton activate <package-name>` or `foton deactivate <package-name>` when you want to change whether an installed package is active
4. Run `foton uninstall <package-name>` to remove a package you no longer need

## Related pages

- [list command reference](../../reference/commands/list.md)
- [info command reference](../../reference/commands/info.md)
- [activate command reference](../../reference/commands/activate.md)
- [deactivate command reference](../../reference/commands/deactivate.md)
- [repair command reference](../../reference/commands/repair.md)
- [uninstall command reference](../../reference/commands/uninstall.md)
