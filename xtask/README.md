# xtask

Development helpers for `foton`.

## Commands

Generate a Windows Sandbox config:

```text
cargo xtask sandbox generate-config --plain
cargo xtask sandbox generate-config --plain --open
cargo xtask sandbox generate-config --scenario <scenario>
cargo xtask sandbox generate-config --scenario <scenario> --open
```

Run a scenario in Windows Sandbox and wait for the result:

```text
cargo xtask sandbox run --scenario <scenario>
cargo xtask sandbox run --scenario <scenario> --timeout <seconds>
```

Run a scenario directly:

```text
cargo xtask scenario run --scenario <scenario> --foton-exe <path> --output-dir <path>
```

## Output

Generated Sandbox config artifacts are written under:

```text
target/windows-sandbox/plain/<run-id>/
target/windows-sandbox/scenarios/<scenario>/<run-id>/
```

Scenario results are written to the specified output directory.

Files:

- `report.json`
- `complete.stamp`
- `<index>.<name>.stdout.txt`
- `<index>.<name>.stderr.txt`
- `<index>.<name>.status.txt`

The numbered files are generated per executed command in run order.

## Notes

- The current implementation assumes `debug` binaries
- Windows uses static CRT linking via `.cargo/config.toml`
