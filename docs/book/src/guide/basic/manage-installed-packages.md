# Managing Installed Packages

This chapter covers the commands you use after packages have already been
installed: `list`, `info`, and `uninstall`.

In the examples below, replace placeholders such as `<package-name>` and
`<version>` with real values.

## List installed packages

Show installed packages:

```console
foton list
```

By default, `list` shows packages in the `installed` state.

If you also want to see packages in pending states, pass `--show-pending`.

```console
foton list --show-pending
```

With `--show-pending`, each entry includes its state, such as `installed`,
`pending-install`, or `pending-uninstall`.

`pending-install` and `pending-uninstall` are transitional states recorded in
the local package database.
If you see packages in pending states, it usually means that an earlier
install or uninstall was interrupted or failed before it finished cleanly.
If you see such packages, you can inspect them with `foton info` and remove
them with `foton uninstall` if needed.
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
This can include packages still recorded in the local package database after an
interrupted or failed install or uninstall.
Use this command when you want to confirm exactly what is recorded in the local package database.

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
`uninstall` can also remove packages still recorded in the local package
database after an interrupted or failed install or uninstall.
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
- [uninstall command reference](../../reference/commands/uninstall.md)
