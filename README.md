# Spindle

A Rust foundation for keeping coding-agent sessions running across API restarts.

Spindle is an independent rebuild informed by [TangleClaw](https://github.com/Jason-Vaughan/TangleClaw). The first milestone implements a **Linux session host, CLI and private HTTP gateway**. It is a development prototype, not a drop-in replacement or a full dashboard.

## What works in this milestone

- Real PTY launch with a structured executable, arguments and working directory.
- Separate terminal host and HTTP gateway: restarting the gateway leaves sessions running.
- Input, terminal resize, byte-cursor replay and explicit output-retention gaps.
- Durable launch IDs, generation checks, exact launch retry and lifecycle records.
- Original Unix process-group cancellation and cleanup when its leader exits.
- Recovery that marks interrupted sessions honestly after a host restart.
- Bounded mailboxes, frames, output and session admission.
- SQLite on a dedicated worker, schema-version checks and transactional recovery.

The terminal host currently requires Linux. A `portable-pty` example exercises the platform abstraction; Windows IPC, Job Objects, services and real-machine validation are still pending. Cross-compilation is not native Windows validation.

## Build and run

Install Rust through [rustup](https://rust-lang.org/tools/install/). The toolchain file pins the version used for development. A C compiler is needed for bundled SQLite.

```sh
cargo build --locked
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --all -- --check
```

Run these in separate terminals from the repository root:

```sh
# Owns the PTYs. Keep this process running.
cargo run --locked -- --state-dir .state host

# Optional HTTP gateway. This process can be restarted independently.
cargo run --locked -- --state-dir .state api
```

Launch a shell through the CLI:

```sh
cargo run --locked -- --state-dir .state launch /bin/sh
cargo run --locked -- --state-dir .state list
```

The launch response includes `launch.id` and `generation`. Use those exact values:

```sh
cargo run --locked -- --state-dir .state input ID GENERATION 'echo hello
'
cargo run --locked -- --state-dir .state output ID GENERATION --after 0
cargo run --locked -- --state-dir .state resize ID GENERATION 40 120
cargo run --locked -- --state-dir .state cancel ID GENERATION
```

Input is acknowledged as **queued**, not delivered to or acted upon by an engine. Output is returned as JSON byte values; use `next` for the following read and handle `gap: true`. There is no terminal emulator/browser UI yet. Do not treat JSON output as a rendered terminal screen.

HTTP uses a private Unix socket, not a TCP port:

```sh
curl --unix-socket .state/api.sock http://localhost/health
curl --unix-socket .state/api.sock http://localhost/v1/command \
  -H 'Content-Type: application/json' -d '{"op":"list"}'
```

The state directory is created with mode 0700 and sockets with mode 0600. An existing public directory, symlinked database or conflicting host is refused. Use a trusted parent directory tree: leaf checks do not prevent another user from replacing writable ancestors. Access is authorized by the local OS user boundary. There is no remote/browser authentication layer, sandbox or privilege separation from programs running as that user. Children inherit the host environment; launch arguments and working directories are persisted locally. Do not place secrets in command-line arguments.

`SIGINT` or `SIGTERM` to the host ends its managed sessions. Stopping only the API does not. Host crash/reboot is a different failure: the record becomes `interrupted`, and memory-only terminal history is unavailable. Never run this prototype against an upstream TangleClaw database.

## Architecture

```text
CLI -------------------+                    +-- PTY child + bounded replay
                       |                    |
HTTP over Unix socket -> private host IPC -> session host
                                            |
                                            +-- bounded store mailbox -> SQLite thread
```

- `src/model.rs`: wire types, validation and byte-ring contract.
- `src/host.rs`: private IPC, launch ownership, PTY workers, cancellation and recovery orchestration.
- `src/store.rs`: versioned storage, legal transitions and atomic recovery.
- `src/api.rs`: replaceable HTTP gateway.
- `tests/lifecycle.rs`: real-process acceptance tests.

This slice uses a native PTY rather than requiring tmux, to exercise the future native-platform boundary early. A tmux compatibility adapter remains planned. Memory replay is limited to 256 KiB per session, output pages to 8 KiB, JSON frames to 64 KiB, concurrent IPC requests and HTTP connections to 64 each (HTTP connections also have a 10-second lifetime), live sessions to 32, and stored records to 128. This last limit is a prototype guard, not a retention policy. Pagination, archival and deletion must precede removing it.

The implementation uses bounded dedicated PTY threads. It does not yet demonstrate lower CPU/RSS than TangleClaw. Measure real matched workloads before making performance claims.

## Known limitations

- No browser terminal, engine-aware launch handshake, coordination, wraps, import, installer or upstream API compatibility yet.
- Cancellation covers the original Unix process group. Descendants that create other groups/sessions require stronger containment such as cgroups; do not claim complete process-tree cleanup.
- Abrupt host termination cannot guarantee orderly descendant cleanup. Recovered records never signal a persisted PID.
- A blocked OS launch call cannot be forcibly cancelled by a Rust future. A startup timeout reports an unknown outcome; reconcile with the **same launch ID**, never blindly create another.
- Output is a bounded in-memory byte stream, not durable transcripts or a VT screen snapshot. Final output drain is bounded; output can be truncated on cleanup.
- Native Windows and macOS host support remain unverified/unimplemented beyond the portable PTY spike.

## Rebuild scope and source review

The compatibility reference is TangleClaw v5.71.0 at `3162811af43714b6d83ef4a43e6602adaead2785`.

- [Session and engine contracts](docs/review-sessions-engines.md)
- [Persistence and coordination contracts](docs/review-coordination-store.md)
- [API, UI, platform and website review](docs/review-api-ui-platform.md)
- [Machine-readable parity requirements](docs/parity-manifest.json)
- [Protocol and acceptance contract](docs/protocol.md)

The reviews inventory the repository and inspect selected critical paths in depth. They do not certify every source line or demonstrate unexecuted platform behavior. The parity manifest deliberately keeps full-product requirements `planned`; passing this milestone's tests does not satisfy an entire parity domain.

Development uses an issue, feature branch and pull request. The milestone is tracked in [issue #1](https://github.com/krishangnaikar/Spindle/issues/1). See [CONTRIBUTING.md](CONTRIBUTING.md).
