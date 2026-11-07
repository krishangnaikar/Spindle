# Validation - 2026-11-07

Executed on Linux x86_64 with Rust 1.99.1, using the checked-in Cargo.lock. Test fixtures use isolated state directories and local shells; no coding-engine account, production TangleClaw store or service installation was used.

| Check | Result |
| --- | --- |
| `cargo test --locked --all-targets` | 101 passed: 71 library, 13 CLI, 10 Linux lifecycle and 7 portable-probe tests; 0 failed, 0 ignored |
| Python unittest discovery | 8 passed, isolated evidence-runner checks |
| `cargo clippy --locked --all-targets -- -D warnings` | Passed |
| `cargo fmt --all -- --check` | Passed |
| `cargo run --locked --example pty_probe` | Linux native PTY spawn/output/exit passed |
| `cargo check --locked --target x86_64-pc-windows-gnu --example pty_probe` | Passed; compilation only, not execution |

The lifecycle suite verifies separate host/gateway processes, gateway kill/restart, output reconnect, input, resize, duplicate launch races, conflicting launch IDs, stale generation, repeated cancellation, retained-output gaps, failed executable persistence, host-restart interruption, original process-group cleanup, oversized frames, duplicate host refusal, filesystem permission/symlink refusal, bounded summaries, graceful shutdown racing launch, HTTP connection admission and slow-header expiry.

The combined suite completed 2026-11-07 at 13:46:31 PST (21:46:31 UTC); its local receipt is `evidence/combined-tests-v2.json`. The earlier 77-test expanded suite is preserved as `units-all`. See [the testing guide](testing.md) for module counts, commands and the distinction between pure unit, isolated component and real-process tests. Earlier foundation validation had 16 tests; that historical count is superseded by this run. Provider tests use local HTTP fixtures, not live accounts.

Storage tests include an injected recovery-write failure and verify transaction rollback. A future schema remains unchanged when refused. These are targeted executable contracts, not full TangleClaw parity or a security certification.

## Failures preserved during development

- Combined CI [42417582825](https://github.com/krishangnaikar/Spindle/actions/runs/42417582825): Linux passed; Windows portable Rust tests passed, then the Python cwd assertion compared a canonical `runneradmin` path with an equivalent short `RUNNER~1` path. The assertion now resolves the expected path as the runner does. This was a test expectation error, not a failed command or missing directory. Windows PTY execution was skipped after that failure; a follow-up run must verify it. The failed-step log is retained separately.


- Strict lint follow-ups found `int_plus_one` in a host assertion and `needless_question_mark` in the prompt reader. Both were corrected without lint suppressions; failures remain in `units-clippy` and `integrated-clippy-v1`, followed by successful `final-clippy`.
- Review found opening a FIFO prompt could block before file-type validation. The Unix prompt reader now uses nonblocking open and descriptor checks, with FIFO/symlink regression coverage. This was a code-review finding, not an observed production hang.
- One host fixture initially inherited a public temporary-parent mode and failed the intended private-directory check. The fixture was made private; the access check remains unchanged. Both runs are retained (`unit-host-verified`, `unit-host-final`).


- Expanded API regression: `{"op":"health","unexpected":true}` returned HTTP 200 rather than 422. Health/List unit enum variants did not enforce the intended unknown-field rejection. Empty struct variants retain the existing wire shape and make the strict schema apply. Protocol and HTTP regression tests now pass; local `unit-api-final.*` and `unit-api-schema-regression.*` preserve the failures, and `unit-schema-fixed.*`/`units-all.*` preserve successful follow-ups.
- First targeted API-test compilation ran before the coordinated development dependencies were added, so Tower/body utilities and Tokio test-time controls were unavailable. This was a test setup ordering failure; the final all-targets run includes the required test-only dependencies.

- First GitHub Actions run [42414666851](https://github.com/krishangnaikar/Spindle/actions/runs/42414666851): Linux passed, but the Windows probe compiled and then remained running without its success marker. The run was cancelled after observation; Windows reported `0xc000013a, STATUS_CONTROL_C_EXIT`, so this is a cancelled/hung attempt, not a passing probe. Dependency inspection showed portable-pty enables inherited-cursor negotiation. The follow-up answers cursor queries, drains output on a separate thread through ConPTY teardown, and adds a 30-second hard deadline. Local Linux execution and Windows cross-check pass; native follow-up results belong to the PR checks. Microsoft documents both cursor-response and teardown-drain deadlock risks in [Creating a Pseudoconsole session](https://learn.microsoft.com/en-us/windows/console/creating-a-pseudoconsole-session). The initial log does not independently isolate which blocked stage occurred.
- Initial formatting attempt: rustfmt was absent; installed the formatter and linter in the project-local toolchain.
- Concurrent toolchain installation collided during the switch from `stable` to pinned `1.99.1`: rustup reported a missing downloaded-file rename, and the next build reported `E0463: can't find crate for std`. Removed and reinstalled only the task-local 1.99.1 toolchain, then reran validation successfully. No source fix was attributed to this environment failure.
- First strict Clippy run rejected a nested `if` in the test readiness helper (`clippy::collapsible_if`). Simplified it and reran successfully.
- Review found overly large list replies, descendant cleanup and shutdown-admission races in the initial implementation. These were fixed before the passing lifecycle suite; no initial runtime reproduction is claimed for those review findings.

The repository's `scripts/check.py` records exact argv, working directory, UTC timestamps, exit codes and raw output locally under ignored `evidence/`. It never dumps the environment. Public PR checks provide a separate execution record. Retain local failures rather than overwriting them when a follow-up changes the setup.

## Limits

No matched Node-versus-Rust resource benchmark has been run. The host has bounded dedicated PTY threads and still polls lifecycle state at a fixed interval; this is a correctness foundation, not proof of reduced idle CPU or memory.

Native Windows probe execution is a separate CI job; its result must be checked before claiming it passed. The Linux cross-check alone proves neither ConPTY runtime behavior nor Windows host support. Windows named-pipe authorization, Job Objects and persistent host implementation remain planned.

Complete browser terminal behavior, real engines, escaped process groups, cgroup containment, host-crash cleanup, machine reboot, database corruption/disk-full behavior and sustained load remain outside this milestone's executed coverage.
