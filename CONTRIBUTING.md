# Contributing

Create or identify an issue before substantive changes. Work on a feature branch and open a pull request against main. Do not push implementation commits directly to main. Keep each PR reviewable, describe behavior and limitations, and link its issue.

Before requesting review:

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

Lifecycle changes require real-process tests for failure and recovery, not only unit tests. Do not claim native Windows validation from a Linux cross-check. Keep runtime state, terminal output, credentials, build directories and local logs out of commits. Document new protocol behavior and track parity against the pinned upstream source.

The repository's initial empty main commit is a one-time bootstrap exception, not the ongoing workflow.
