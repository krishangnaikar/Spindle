# Testing Spindle

Run the complete Linux suite from the repository root:

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo run --locked --example pty_probe
```

`--all-targets` includes example unit tests as well as the library, CLI and lifecycle suite. Running the example separately exercises a real native PTY; its parser tests alone do not.

## Combined CLI/API milestone

The combined Linux suite passed **101 Rust tests** on 2026-11-07 at 21:46:31 UTC: 71 library, 13 CLI, 10 real-process lifecycle and 7 probe tests. All 8 Python runner tests also passed. Receipts: `combined-tests-v2`, `combined-python-v2`. Final Clippy and formatting checks pass (`final-clippy`, `final-format-check`); Linux native PTY execution passes (`combined-probe-v2`).

The library adds 7 native-engine cases and 12 provider cases to the prior 52. Native-engine cases exercise Claude/Codex construction, model/ID/path validation and literal arguments. Provider cases use local mock TCP servers for request formats, free routing, status errors, credential-safe failures, refused redirects, exact/oversized/chunked responses and header/body deadlines. Parser cases distinguish completion, truncation, refusal and unsupported tools. Five additional CLI cases cover engine selection, paid-route opt-in, key-variable names, prompt limits and special-file refusal. Existing parser tests also verify explicit engine ID/cwd forwarding. No external inference or subscription login was used.

CI runs all Linux targets and the Windows portable library/CLI/probe subset, plus Python checks and native PTY execution on both platforms. Native CI results must be read for the corresponding commit; the Linux result is not a Windows claim.

## Measured coverage by area

The Linux run completed on 2026-11-07 at 13:38:50 PST (21:38:50 UTC), using Rust 1.99.1 and Cargo.lock. All **77 tests passed**, with no failures or ignored tests: 52 library tests, 8 CLI tests, 10 real-process lifecycle tests and 7 probe tests. This is a dated result, not a promise that later edits or other machines were tested. Exact command and output are recorded locally in `evidence/units-all.json` and `evidence/units-all.log`.

| Area | Cases | What is exercised |
| --- | ---: | --- |
| Model and protocol | 11 | Launch identity/path/argument limits, inclusive size boundaries, request round trips, unknown/missing/out-of-range fields, reply envelopes, output pagination, retention gaps, EOF and deterministic reference-stream comparisons |
| Persistence | 13 | All status-pair transitions, duplicate generation reuse, exact terminal replay, malformed/missing records, disk reopen, interrupted recovery, transaction rollback, future-schema refusal and database-worker failures/concurrency |
| Host contracts | 16 | Private paths and locks, socket ownership, frame size/time bounds, generation checks, queue refusal, executor receipts and unknown outcomes, launch stop/admission fences and bounded summaries |
| HTTP gateway | 12 | Pre-parse connection admission, permit lifetime, byte forwarding, absolute IO deadlines, no trickle extension, host success/error forwarding, health, JSON/schema/media/method/path refusal and body limits |
| CLI | 8 | Help/version, subcommands, state-directory placement, required/numeric arguments, literal child arguments, working-directory identity and structured request construction |
| Lifecycle | 10 | Real host/gateway/PTY processes, gateway restart/reconnect, input/resize, concurrent launch, durable failure, cancellation, original process-group cleanup, shutdown races, output flooding and idle HTTP connections |
| Portable probe | 7 | Cursor query and marker detection across chunk splits, bounded parser retention, reply queue refusal, repeated queries and interrupted/broken/error reads |

Some library tests use temporary SQLite files, local Unix socket pairs/listeners and fake executor channels. They are isolated component tests, not all pure functions. Fake host responses test gateway forwarding without launching a real host. Paused-clock tests verify timeout behavior without sleeping for the full deadline. Lifecycle tests supply the separate real-process evidence.

The deterministic byte-stream test covers many chunk/cursor cases against a reference model. It is not a proof over all possible inputs, a fuzzing campaign or a race detector. A test count is not a code-coverage percentage.

## Focused commands

```sh
cargo test --locked --lib model::
cargo test --locked --lib store::tests
cargo test --locked --lib host::tests
cargo test --locked --lib api::tests
cargo test --locked --bin spindle
cargo test --locked --example pty_probe
cargo test --locked --test lifecycle
```

For Windows, run the portable subset and the native PTY probe:

```sh
cargo test --locked --lib --bin spindle --example pty_probe
cargo run --locked --example pty_probe
```

Platform-specific cases are gated at compilation. Counts therefore differ from Linux. Windows CI explicitly runs this subset; Unix API/host/storage tests cannot establish Windows behavior. A Linux cross-compilation check is useful but does not replace native execution. The Windows session host, named pipes, Job Objects and service integration remain outside implemented scope.

## Regression found by the expanded suite

A request carrying `{"op":"health","unexpected":true}` originally reached the HTTP handler and returned 200 instead of the expected 422. Serde's internally tagged unit variants did not enforce the intended unknown-field rejection. Health/List now use empty struct variants, preserving their wire shape while rejecting extra fields. Protocol and HTTP regression cases cover both operations. The original failing runs are preserved locally as `unit-api-final.*` and `unit-api-schema-regression.*`; `unit-schema-fixed.*` and `units-all.*` record successful follow-ups.

## Evidence and limits

Use `python3 scripts/check.py NAME COMMAND...` for a local receipt with exact argv, directory, UTC timestamps, exit code and raw output. Choose a new NAME for changed setups so failures remain available. The ignored `evidence/` directory is local evidence; GitHub Actions logs are a separate shared record. Do not publish secrets, real terminal transcripts or environment dumps.

Current tests do not exhaustively exercise all branches, interleavings, allocation failures or OS failures. Coverage still excludes real coding engines and their accounts, browser/mobile terminal behavior, upstream import/parity, native Windows/macOS persistent hosts, escaped process groups, host crash/reboot containment, sustained load, database corruption/disk-full fault injection and matched performance benchmarks. Preserve these gaps rather than describing the suite as testing everything.

## Additional runner and instrumented coverage evidence

A subsequent Linux check on 2026-11-07 completed **8 Python check-runner tests** successfully (`evidence/unit-python-runner-v1.*`, finished 13:42:19 PST). These exercise receipt and log handling, subprocess outcomes and failures; they are separate from the 77 Rust-test snapshot above. CI runs discovery on Linux and Windows:

```sh
python -m unittest discover -s tests -p "test_*.py" -v
```

The earlier `cargo llvm-cov` 0.9.1 all-targets report measured **2,194 of 2,283 lines (96.10%)** in the reported files (`src/api.rs`, `src/host.rs`, `src/main.rs`, `src/model.rs`, `src/store.rs`). The report includes inline unit-test lines and excludes the probe and separate integration-test sources. It is therefore **not product-only coverage**, not full-repository coverage and not evidence of all branches or race schedules. The local artifact is `evidence/coverage-report.json`. It predates the provider-routing additions and must not be presented as their coverage; a new measured result needs its own scope and timestamp.

## Final measured line coverage

`cargo llvm-cov` 0.9.1 reran all 101 Rust tests successfully at 2026-11-07T21:47:20 UTC. The final report records **95.57%** (3217/3366) lines covered across the files below. This is LLVM's source-file aggregate, including inline test code. The default report excludes `examples/pty_probe.rs` and standalone test files; Python is outside LLVM coverage. It is not production-only or branch/exhaustive coverage.

| File | Covered/total lines | Line coverage |
| --- | ---: | ---: |
| src/api.rs | 374/385 | 97.14% |
| src/engine.rs | 155/155 | 100.00% |
| src/host.rs | 851/913 | 93.21% |
| src/main.rs | 457/495 | 92.32% |
| src/model.rs | 247/247 | 100.00% |
| src/provider.rs | 604/639 | 94.52% |
| src/store.rs | 529/532 | 99.44% |

Reproduce with `cargo llvm-cov --locked --all-targets --json --output-path evidence/final-coverage-report.json` after installing llvm-tools-preview and cargo-llvm-cov0.9.1. The command receipt is `final-coverage.json`; do not use the same filename for the LLVM report and runner receipt. No coverage percentage is a substitute for the case matrix or platform checks.
