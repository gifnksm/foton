# xtask

Development helpers for `foton`.

## Commands

Generate a Windows Sandbox config:

```text
cargo xtask sandbox generate --scenario <scenario>
```

Run a scenario directly:

```text
cargo xtask scenario run --scenario <scenario> --foton-exe <path> --output-dir <path>
```

## Output

Generated Sandbox configs are written under:

```text
target/windows-sandbox/scenarios/<scenario>/
```

Scenario results are written to the specified output directory.

Files:

- `stdout.txt`
- `stderr.txt`
- `exitcode.txt`
- `result.txt`

## Notes

- the current implementation assumes `debug` binaries
- Windows uses static CRT linking via `.cargo/config.toml`
