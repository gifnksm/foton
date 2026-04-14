# xtask

Development helpers for `foton`.

## Commands

Generate a Windows Sandbox config:

```text
cargo xtask sandbox generate-config --scenario <scenario>
```

Run a scenario directly:

```text
cargo xtask scenario run --scenario <scenario> --foton-exe <path> --output-dir <path>
```

## Output

Generated Sandbox configs are written under:

```text
target/windows-sandbox/scenarios/<scenario>/<run-id>/
```

Scenario results are written to the specified output directory.

Files:

- `report.json`
- `<index>.<name>.stdout.txt`
- `<index>.<name>.stderr.txt`
- `<index>.<name>.status.txt`

The numbered files are generated per executed command in run order.

## Notes

- the current implementation assumes `debug` binaries
- Windows uses static CRT linking via `.cargo/config.toml`
