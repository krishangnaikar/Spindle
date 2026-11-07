# Protocol v1 and lifecycle contract

Development contract, not upstream API compatibility. The same Request JSON is accepted over `host.sock` (one newline-terminated frame per connection) or `POST /v1/command` on `api.sock`. The host is authoritative; the gateway keeps no session ownership.

## Requests

| op | Fields | Behavior |
| --- | --- | --- |
| health | none | Host/service protocol identity |
| launch | launch: {id, program, args, cwd} | UUID ID, executable, argument array, absolute cwd |
| list | none | Bounded summaries: id, generation, status, exit_code, error |
| output | id, generation, after | Read bytes from a cursor within this host lifetime |
| input | id, generation, data | Queue byte array, maximum 16 KiB |
| resize | id, generation, rows, cols | Dimensions must be 1..1000 |
| cancel | id, generation | Persist cancellation intent and force-stop original process group |

Replies are `{ok:true,value:...}` or `{ok:false,error:"..."}`. Errors are currently text, not stable machine error codes. HTTP JSON/schema/body errors can use HTTP error statuses; valid command failures use the Reply envelope. No batch protocol or streaming subscription exists yet.

`launch.id` is the idempotency key. Identical replay returns the existing generation and outcome; reusing it for a different specification fails. A stopped or interrupted ID is not a request to relaunch. Every control operation must match the stored generation. Input/resize do not yet have durable per-command deduplication.

Launch is persisted as `starting` before creating a PTY. Successful creation persists `running` before readiness is acknowledged. Cancel persists `cancelling` before signalling. Exit/failure is recorded separately. Legal transitions are validated in the store; terminal outcomes cannot be rewritten. A replay of an identical stored transition is harmless. On host restart, active states become `interrupted` transactionally, and no PID from disk is signalled.

## Replay

`output` returns `{start,next,gap,bytes,eof}`. Cursors count bytes, not UTF-8 characters or terminal cells. A cursor ahead of the stream is refused. A cursor older than the retained range gets `gap:true` and starts at the earliest retained byte. `next` resumes after this page. `eof` is only true when this page reaches the end of the currently closed stream.

Replay is memory-only and limited to 256 KiB per session. Restarting the API preserves replay; restarting the host loses it and refuses output requests for those prior sessions. The caller must not silently pretend a replay gap is complete output.

## Trust and limits

Private Unix sockets and mode-0700 state directories provide the current local-user access boundary. The parent directory tree must be trusted; validation of the final directory does not anchor writable ancestors against replacement. This is an operator tool with the user's shell authority. There is no browser/network listener. The first prototype inherits host environment for engine compatibility; an explicit environment contract is future work.

No raw shell command is assembled by Spindle. Shell interpretation occurs only when the caller explicitly launches a shell with its own arguments. PTY output is returned as bytes and is not evaluated.

Limits are enforced at launch, frame, input, output, mailbox and admission boundaries. A timed-out request may have reached the host; reconcile instead of treating timeout as proof of no effect. Lifecycle persistence does not promise exactly-once behavior for external programs.

## Acceptance evidence

`cargo test --locked` exercises actual Linux PTYs, separate API and host processes, restart/reconnect, resize/input, launch replay/conflicts, stale cancellation, replay gaps, original-group cleanup, malformed frames, socket ownership and storage recovery. Tests use temporary state directories and controlled local shell fixtures. They do not launch a real coding engine or validate native Windows.

`cargo run --locked --example pty_probe` exercises native PTY spawn/output/exit. The same example can be run on Windows, but Windows execution evidence must be collected on Windows before claiming support.
