# Copilot review instructions

When reviewing pull requests in this repository:

- Prefer actual compile/build/test results over speculation for compilability concerns.
- If actual results are available, including GitHub Actions results, treat them as the source of truth.
- If no actual result is available, only claim that code does not compile when the error is obvious from the diff; otherwise describe it as a hypothesis.
