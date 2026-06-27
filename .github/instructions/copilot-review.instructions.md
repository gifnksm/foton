# Copilot review instructions

When reviewing pull requests in this repository:

- Prefer actual compile, build, test, format, and lint results over speculation for concerns that are already checked automatically.
- Treat GitHub Actions and other repository checks as the source of truth for formatting, import ordering, compilation, clippy, and tests.
- Do not leave review comments that only predict `cargo fmt`, import ordering, compilation, or test failures when those checks have not actually failed.
- It is still useful to point out style or formatting issues that `cargo fmt` does not normalize well, especially around macro invocations, attribute macros such as `#[...]`, or other cases where the formatter cannot automatically produce the preferred result.
- If no actual result is available, only claim that code does not compile when the error is obvious from the diff; otherwise describe it as a hypothesis rather than a defect.
- In Rust code that uses `snafu`, do not assume selector field types must match exactly. Generated selectors often accept values through `Into`, so references such as `&Path` for `PathBuf` fields or `&str` for `String`/`OsString` fields may be valid. Do not leave review comments that predict selector type mismatches unless compiler output has actually shown a problem.
- Focus review comments on logic, correctness, behavior, maintainability, API design, security, and other issues that automated checks do not already cover well.
