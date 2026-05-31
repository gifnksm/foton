# Managing Installed Packages

This chapter covers the commands you use after packages have already been
installed: `list`, `info`, and `uninstall`.
It also explains how to recover from incomplete operations with `repair`.

In the examples below, replace placeholders such as `<package-name>` and
`<version>` with real values.

## List installed packages

Show installed packages:

```console
foton list
```

By default, `list` shows packages in the `installed` state.

If you also want to see packages left by incomplete operations, pass
`--show-incomplete`.

```console
foton list --show-incomplete
```

With `--show-incomplete`, each entry includes its state, such as `installed`,
`install-incomplete`, or `uninstall-incomplete`.

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

`info` prints the package name, version, state, metadata, and source
information for matching packages recorded in the local package database.
This can include packages left by incomplete operations.
Use this command when you want to confirm exactly what is recorded in the local package database.

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
3. Run `foton uninstall <package-name>` to remove a package you no longer need

## Related pages

- [list command reference](../../reference/commands/list.md)
- [info command reference](../../reference/commands/info.md)
- [repair command reference](../../reference/commands/repair.md)
- [uninstall command reference](../../reference/commands/uninstall.md)
