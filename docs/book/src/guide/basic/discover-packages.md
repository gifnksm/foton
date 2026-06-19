# Discovering Packages

Use `foton search` to find packages that are available in your registries.

In the examples below, replace placeholders such as `<query>` and
`<registry-id>` with real values.

## Basic search

Search for a package by name:

```console
foton search <query>
```

Search with multiple query terms:

```console
foton search <query-word-1> <query-word-2>
```

Each query term must match within the same package metadata field.
In practice, this means `foton` can match package names, display names,
aliases, individual font titles, and descriptions.

## Restrict the search to specific registries

If you want to search only selected package registries, pass `--registry`
with a comma-separated list of package registry IDs.

```console
foton search --registry <registry-id-1>,<registry-id-2> <query>
```

## Control the number of results

By default, `foton search` shows up to 10 matching packages.
Use `--limit` to change that number.

```console
foton search --limit 20 <query>
```

## Read the results

Search results are printed as package names with versions together with the
registry ID that provided the package.
If a package has a description, it is shown on the next line.

Example output:

```text
example-font@1.2.3 [example]
  Example font family for UI and coding
```

By default, `search` does not include pre-release versions when it
selects the latest version of each package in each selected registry.
Use `--pre-release` if you want search results to include pre-release
versions.
Once you have found a package you want, install it with `foton install`.

## Related pages

- [Installing and Updating Packages](install-update-packages.md)
- [search command reference](../../reference/commands/search.md)
