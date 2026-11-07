# Contributing

Create or identify an issue before substantive changes. Work on a feature branch and open a pull request against main. Do not push implementation commits directly to main. Keep each PR reviewable, describe behavior and limitations, and link its issue.

Before requesting review:

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
```

Lifecycle changes require real-process tests for failure and recovery, not only unit tests. Do not claim native Windows validation from a Linux cross-check. Keep runtime state, terminal output, credentials, build directories and local logs out of commits. Document new protocol behavior and track parity against the pinned upstream source.

The repository's initial empty main commit is a one-time bootstrap exception, not the ongoing workflow.

Use [the testing guide](docs/testing.md) to select focused checks while developing. Every production module has targeted contract tests; add regression cases for changed behavior and boundaries instead of mirroring private implementation details. Socket, SQLite and filesystem component tests belong alongside pure unit tests but must be identified accurately. Keep Windows-portable tests free of Unix-only dependencies. The Windows CI job runs portable library, CLI and probe tests followed by a real native PTY probe; it does not validate a Windows session host.
