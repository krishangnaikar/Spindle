# Session, engine, launch and terminal review

Reviewed November 7, 2026 (America/Los_Angeles). Reference source: TangleClaw `3162811af43714b6d83ef4a43e6602adaead2785` (5.71.0). This report defines requirements for Spindle; it does not claim feature parity or tested compatibility.

## Coverage and evidence limits

The review independently inspected the lifecycle, launch, command injection, launch acknowledgements, control transitions, activity capture, engine configuration and native startup control paths. It used the earlier repository-wide audit as context. Source anchors below refer to the pinned upstream revision, not Spindle files. Resolve any path and line with `https://github.com/Jason-Vaughan/TangleClaw/blob/3162811af43714b6d83ef4a43e6602adaead2785/<path>#L<line>`.

| Area | Files inspected and useful entry points | Depth |
| --- | --- | --- |
| Session lifecycle | `lib/sessions.js:343` launch, `:4145` status, `:4651` inject, `:6146` finalize, `:6360` kill, `:8478` restart reconciliation | Selected full functions and adjacent invariants in this large module |
| Persistence transitions | `lib/store/sessions.js:81`, `:130`, `:177`; `lib/store/launch.js` | Transition implementation and launch-store boundary |
| Terminal transport | `lib/tmux.js:65`, `:200`, `:261`, `:647`, `:982`, `:1187`, `:2264`, `:2450` | Sync/async helpers, launch identity and input protection |
| Observation | `lib/activity-observer.js:76`, `:135`, `:260`, `:318`; `lib/pty-activity.js:62` | Constants, capture sequence, fairness and freshness |
| Engine capabilities/config | `lib/engines.js:604`, `:641`, `:1451`, `:1932`, `:2130`, `:3929`, `:4190`; `lib/engine-admission.js:1` | Detection, mode reconciliation, managed configuration and admission primitive |
| Native startup | `lib/startup-control.js:169`, `:253`, `:403`; `lib/startup-control-codex.js:537`, `:866`, `:965`, `:1079`, `:1531`, `:2057`, `:2424`, `:2760`, `:3087` | Contract and selected launch/readiness/recovery implementation; not every protocol branch |
| Launch context | `lib/launch-sequence.js:574`, `:697`, `:1123`, `:1714`, `:1831`; `lib/launch-preflight.js:424`; `lib/launch-binding-guard.js:321` | Snapshot/ACK/READY gates and bindings |
| Safe input | `lib/startup-dialog.js:976`, `:1125`, `:1254`, `:1390`; `lib/launch-pane-writer.js:1`; `lib/launch-kickoff.js:330`; `lib/launch-unready.js:154` | Dialog recognition and one-shot send semantics |
| Governance | `lib/control-state.js:678`, `:735`, `:801`; `lib/control-gate.js:237`; `lib/session-finalize.js:1`, `:276` | HOLD/release/STOP and guarded finalization |
| Terminal service | `lib/ttyd-attach.js:20`, `:54`; `lib/ttyd-watcher.js:277`, `:433`, `:482`; `lib/ttyd-runtime.js:196`, `:415`, `:454`; `lib/ttyd-bind.js:151` | Lifecycle interfaces, platform assumptions and recovery receipts |
| Supporting scope inventory | `launch-*`, `session-*`, `startup-*`, `engine-*`, `control-*` modules; ttyd build/deploy scripts; `docs/adr/0017-phased-launch.md`, `0018-owned-ttyd-runtime.md`, `0025-codex-cold-start-wake-and-pane-refusal.md` | File/function inventory and selective contracts, not exhaustive line review |

The filename-based test inventory matched 192 paths under `test/` for launch/session/engine/tmux/startup/control/activity-observer/pty/ttyd. This includes fixture matches and is not a count of tests executed. This review ran no new runtime tests, installed no engine, started no terminal service and changed no upstream source. Earlier audit test totals are separate evidence, not validation of Spindle. Browser terminal interaction, remote OpenClaw, native Windows, macOS TCC, and real engine-version compatibility remain untested here.

## Contracts to preserve

### A launch is a transaction across resources

`launchSession` refuses archived projects, stopped assignments, unavailable engines, malformed profile identities and applicable preflight blockers. Existing session liveness has three outcomes. A timed-out probe returns `LIVENESS_UNKNOWN` and does not mark the session dead or launch another one (`sessions.js:343`, `tmux.js:261`). Spindle must preserve unknown separately from absent.

Launch creates more than a process: configuration, a launch identity, preflight/snapshot state, an optional private app-server, a terminal, a database session, model admission and control bindings. Native-channel preparation precedes the terminal; failure to persist the session after spawn invokes cleanup (`sessions.js:850` onward). This cannot be made atomic with a single database transaction. Use durable intent and explicit compensation/reconciliation, including the state where process creation succeeded but the response was lost.

Record the effective launch mode, not merely the requested mode. The upstream launch path reconciles unsupported modes before persisting them. Model selection is another admission gate: ordinary injected work is held while post-launch model verification is pending (`sessions.js:4651`, `:6742`). A successful spawn is not proof the requested model is running.

### Context receipt is distinct from permission

The four-stage sequence binds identity, governance, state and task to a revision and exact content. ACK validates revision before replay handling, verifies digest, requires current ordering and all pages served (`launch-sequence.js:1123`). READY validates preflight, recovery mode, acknowledgements and required reconciliation (`:1714`). READY attests initialization; it grants neither task authorization nor authentication.

Represent process state, initialization state, task admission and control state independently. A single `Running` enum cannot express all four. Keep stale launch IDs from mutating replacement sessions. The exact byte/digest and revision contracts are compatibility requirements if the original command protocol is retained.

### A terminal write is a potentially destructive operation

Upstream injection checks project/session binding, active state, web UI capability, control holds, startup dialogs, active wraps and model admission (`sessions.js:4651`). Dialog detection is checked again close to the write because a dialog can appear between observations. Unknown or partially drawn startup UI must not be interpreted as an empty composer.

`tmux.js:1187` saves identifiable drafts privately before clearing, avoids logging text, and distinguishes its own stranded nudges from user drafts. It also clears when a draft cannot be located, recording the loss of evidence. That is an upstream behavior to examine explicitly, not a safety guarantee to copy unquestioned. For Spindle, prefer refusal or explicit user-directed replacement when composer contents are unknown. Keep raw operator keystrokes separate from automated prompt injection; they require different policy.

Kickoff claims a fire-once slot before sending and releases only on a pre-send bailout; the unready monitor counts its nudge after successful send (`launch-pane-writer.js:1`). A lost response after input was accepted is indeterminate. Blind retries can duplicate work. Persist attempt IDs and reconcile where adapters support it.

### Engine adapters are more than command templates

The native Codex implementation already creates a per-launch app-server, attaches the terminal, verifies executable/version/isolation and reconciles thread activity. It combines protocol evidence with terminal evidence (`startup-control-codex.js:866`, `:1531`, `:2424`). Do not replace it with prompt-pattern matching alone or assume all versions implement the same behavior.

Capability declarations cover configuration carriers, launch modes, model selection, prompt delivery and local versus remote operation. Preserve user-managed configuration outside managed blocks; engine switching includes retiring inactive carriers and hooks (`engines.js:501`, `:3929`, `:4520`). Aider's bounded task runner and remote web UI sessions are distinct execution modes, not ordinary PTY sessions with a different executable.

`engine-admission.js` contains a rotation/epoch primitive with compare-and-set persistence and fail-closed file-lock contention. Its header explicitly separates this primitive from dispatcher/session integration. Do not infer that the entire application enforces publication epochs merely because that helper exists.

### Control and retirement must survive races

HOLD creates an event and hold inside the state transaction. Release requires an exact generation and explicit active hold IDs; releasing one hold does not release others. STOP is terminal until a successor assignment is established (`control-state.js:678`, `:735`, `:801`). Notification outcomes must not undo the committed control decision.

The upstream store enforces terminal transitions in SQL (`store/sessions.js:130`, `:177`). Cleanup may fail after a durable terminal status is written (`sessions.js:6360`), so record surviving resources and reconcile them instead of reporting that every process definitely ended.

Headless finalization is much stronger than closing a terminal: authority, current binding, no hold, no running wrap, safe-to-clear workload evidence, drained obligations, checkout/worktree checks and no owned unpublished work (`session-finalize.js:1`, `:276`). Keep forced cancellation and successful project completion as different operations.

## Efficiency and platform implications

The observer uses a 10-second tick, 1-second capture deadline, 3-second whole-tick budget and 30-second freshness window. It rotates fairly and exposes stale observations as unknown. A capture issues three or four tmux commands (`activity-observer.js:135`). Existing asynchronous/coalesced terminal helpers are useful precedent; centralize observations and avoid rebuilding per-client polling over new Rust wrappers.

Keep all process waits and potentially blocking PTY/file operations off API workers. Bound terminal output, queued input, retained sessions, client subscriptions and concurrent launches separately. A bounded output ring does not bound retained database history or total connection count. Specify lag/gap and EOF semantics; reconnecting clients need byte cursors or sequenced events, not an assumption that output is complete.

The current ttyd deployment has macOS-specific TCC paths, launchd recovery and PTY pool checks. Its managed binary has provenance verification and rollback. Spindle can replace this machinery with a native host, but must retain observable recovery outcomes. Test Linux process groups and Windows ConPTY/job cleanup independently. API restart survival is different from terminal-host crash survival; do not describe the latter as supported until proven.

## Current Spindle foundation observations

Snapshot inspected during this review: `src/model.rs`, `src/lib.rs`, `src/store.rs` in the work in progress. These observations may be superseded by the main implementation before the PR is finalized.

Positive foundations: generation checks on protocol mutations, bounded output retention, explicit replay gaps, direct program/argument representation, a dedicated SQLite thread with bounded mailbox, and an honest `Interrupted` state on host restart. The store does not signal a persisted PID, avoiding PID-reuse cleanup by assumption.

Priorities for the initial slice:

1. Enforce legal state transitions at the store boundary. The inspected `Store::set` checks generation but accepts arbitrary status changes. A stale completion or cancellation task must not revive a terminal session or replace an established outcome.
2. Make replay of a launch ID return its existing outcome without spawning a second process. Test identical and conflicting requests, including simultaneous requests and lost responses.
3. Verify that API restarts do not kill sessions, while host loss produces truthful interrupted state. Document output history loss on host restart if the ring is memory-only.
4. Verify cancellation of the intended process tree, delayed output drain, repeated cancellation and completion races. An HTTP success or stored terminal state is not proof all descendants exited.
5. Keep startup schema handling forward-safe. The inspected store unconditionally sets `user_version=1`; reject a newer unsupported schema before writes rather than silently lowering its recorded version.
6. Replace whole-history JSON scans for individual lookup/update as history grows. Current create/set call `list` and deserialize every record. Add indexed point lookup and pagination before removing the prototype fleet cap.

Engine-aware initialization, governed input, durable launch attestation and full platform adapters are later milestones. Their absence should be explicit in the first PR, rather than hidden under a claim of full TangleClaw compatibility.

## Acceptance cases and sequencing

| Priority | Case | Required observable outcome |
| --- | --- | --- |
| P0, first slice | Duplicate/concurrent launch ID, same and different payload | At most one process; same result on exact replay; conflicting payload rejected |
| P0, first slice | API process restarts while PTY emits output | Session survives; reconnect cursor gives retained output or explicit gap |
| P0, first slice | Host dies during starting/running/cancelling | No invented successful continuation; interrupted record and documented cleanup limits |
| P0, first slice | Exit races cancel; duplicate cancel; stale generation | Legal monotonic final status; replacement untouched |
| P0, first slice | Flood output and slow/no client reads | Bounded memory; explicit truncation/gap; host remains responsive |
| P0, first slice | Failed executable, invalid cwd, DB failure after launch intent | Stable failed outcome; no untracked surviving child on handled failures |
| P0, first slice | Newer database schema | Refusal before mutation; version preserved |
| P1, engine layer | Busy, unknown, partial dialog, existing draft at automated send | No unsolicited destructive send; explicit reason and retry policy |
| P1, engine layer | Context revised after pages served, stale ACK/READY replay | Revision conflict; no false readiness or authority |
| P1, engine layer | Requested launch mode/model unsupported or unverified | Effective mode reported; ordinary work gated until verification |
| P1, control | Concurrent holds, stale release, notification failure, restart | Durable holds remain; only authorized named releases apply |
| P1, platform | Native Windows launch/resize/reconnect/cancel and Linux process tree tests | Platform-specific executor evidence, no cross-platform assumption |
| P2, parity | Native adapter reconnect/version mismatch/unknown turn outcome | Reconcile or explicitly refuse; never silently duplicate startup turn |
| P2, parity | Remote web UI session and bounded one-shot engine task | Capability-correct operations and honest unsupported responses |
| P2, parity | Finalization with unpushed commit, stash, new worktree or pending message | Refusal with actionable evidence; no loss through automatic retirement |

These priorities stage delivery, not final scope reduction. Preserve complete behavior requirements in issues, and attach actual executor evidence to each implemented acceptance case.

## Implementation follow-up

The foundation observations above describe the initial review snapshot. Before the first PR, Spindle added forward-schema refusal, indexed create/update lookups, legal transitions, bounded list replies, shutdown admission fencing and original-process-group cleanup. See [validation.md](validation.md) for executed evidence and remaining limits. These fixes do not satisfy the complete upstream parity requirements.
