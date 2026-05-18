# Installing Foton

`foton` is supported on Windows only.

## Install a pre-built binary

Pre-built binaries are published on the [GitHub Releases page](https://github.com/gifnksm/foton/releases).

If you already use [`cargo-binstall`](https://github.com/cargo-bins/cargo-binstall), you can install `foton` with:

```console
cargo binstall foton
```

If you prefer, you can also download a release archive manually from the Releases page.
When installing this way, make sure `foton.exe` is placed in a directory on
your `PATH`, or add the extraction directory to your `PATH` yourself.

## Install with Cargo

To install `foton` with Cargo, first install the Rust toolchain.
See [the Rust installation guide](https://www.rust-lang.org/tools/install) if you do not have Rust yet.

Then install either the latest released version or the current Git version from the [GitHub repository](https://github.com/gifnksm/foton).

```console
cargo install foton
```

```console
cargo install --git https://github.com/gifnksm/foton.git foton
```

## Verify the installation

Run:

```console
foton --help
```

If the command succeeds, `foton` is installed and available on your `PATH`.

## Next steps

If you are new to `foton`, read [Core Concepts](core-concepts.md) first.
Then continue with [Basic Usage](basic/README.md) to learn the everyday
workflow for searching, installing, updating, inspecting, and removing
packages.
