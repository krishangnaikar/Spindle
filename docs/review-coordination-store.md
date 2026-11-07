# Coordination, persistence, and workflow migration contracts

Review date: 2026-11-07 (America/Los_Angeles). Source: TangleClaw commit `3162811af43714b6d83ef4a43e6602adaead2785`, version 5.71.0. All source references below are relative to that immutable upstream snapshot, not Spindle's future implementation.

## Scope and evidence limits

This review inspected the persistence facade and transaction helpers, schema and selected migration/postcondition paths, control authority and mutation gates, workload receipt/composition contracts, messaging exchange and listener recovery, handoff publication and lifecycle coordination, rule refresh, and wrap/commit behavior. It used source inspection and targeted searches; it did not execute fault injection, real message delivery, Git automation, or database migrations. Comments describing previous empirical tests are upstream claims, not newly repeated results. This is a subsystem contract review, not proof that every line or every migration was checked.

Core files inspected: `lib/store.js`, `lib/store/_db.js`, `lib/store/_schema.js`, `lib/control-auth.js`, `lib/control-state.js`, `lib/control-gate.js`, `lib/workload.js`, `lib/workload-compose.js`, `lib/medusa-exchanges.js`, `lib/medusa-listener.js`, `lib/handoff-publication.js`, `lib/handoff-lockfile.js`, `lib/handoff-publish.js`, `lib/lifecycle-handoff.js`, `lib/session-rules-channel.js`, `lib/session-rules-refresh.js`, `lib/wrap-pipeline.js`, `lib/wrap-default-pipeline.js`, and `lib/wrap-steps/commit.js`. Their referenced daemon, route, bridge, registry, and individual step implementations require follow-up execution and deeper branch coverage.

## 1. Persistence is more than a SQLite schema

`lib/store/_schema.js:64` sets schema version 80. `lib/store/_db.js:85` provides nested savepoint semantics, while `:114` serializes read-check-write decisions with `BEGIN IMMEDIATE`. Rust code should expose domain transactions through a dedicated database owner; converting synchronous calls into independently scheduled async queries would split decisions that are atomic today.

Migration validation must inspect stored constraints and triggers, not merely columns. Examples include append-only launch recovery clearances (`lib/store/_schema.js:4193`, `:4232`), row-count verification after table rebuild (`:4334`), and append-only exchange facts (`:5141`). Preserve these guarantees or replace them with explicit, tested equivalents. Importing rows into permissive tables is not feature parity.

There is also domain-owned schema outside the central versioned file: `lib/lifecycle-handoff.js:7` installs its own tables additively. A migration inventory derived solely from `_schema.js` misses data. Enumerate actual `sqlite_master` tables, indexes, triggers, foreign keys, and domain-owned initialization paths.

Configuration ownership differs by file. `lib/store.js:349` initializes directories and configuration; `:396` onward distinguishes canonical bundled engine profiles from operator-owned orchestration profiles. Preserve operator edits where ownership is seed-once; do not apply one overwrite policy to every configuration file. Several facade writes are direct `writeFileSync` operations (`:631`, `:715`, `:842`), so power-loss safety should be specified rather than assumed from the presence of SQLite.

**Spindle contract:** use a separately versioned database and offline importer, preserve original data, classify all external files by ownership, and produce an import report with counts, unresolved references, constraints, and unsupported records. Never run both products as writers against the same database.

## 2. Control authority must survive lost notifications

The central guarantee is committed control before notification (`lib/control-state.js:4`): the command and `notify_pending` receipt are written in one transaction. A failed notification cannot undo HOLD or STOP.

Hidden invariants:

- One open assignment per project. STOP stays authoritative until an operator atomically creates its successor (`:600`). Closing or losing a listener must not make the project ungoverned.
- HOLDs accumulate as independently named records. RELEASE names the exact holds and observed generation (`:727`); stale releases cannot clear newer holds. `hold()` increments generation (`:694`), and `release()` compares it before mutation (`:762`).
- State generations move on create/hold/release/stop, not receipt delivery or rebinding (`:18`). Keep state version and delivery progress separate.
- Identity derives from a verified actor and assignment authority, never a role label or message sender. `lib/control-auth.js:4` distinguishes account-session proof from deliberately open authentication. Fallback/unreadable gate states do not constitute operator proof.
- The mutation gate reads durable control, not inbox contents (`lib/control-gate.js:4`). The wrap runner checks control again at step boundaries (`lib/wrap-pipeline.js:490`), and commit automation rechecks before push (`lib/wrap-steps/commit.js:711`). A single check at request entry is insufficient for long workflows.

**Spindle contract:** typed principals and commands, transactional generation checks, durable notification outbox, and revalidation before each irreversible external action. Do not promise to interrupt arbitrary already-running engine commands merely because a governed mutation gate is held.

## 3. Workload assertions are not activity observations

`lib/workload.js:4` stamps session, launch, assignment, sequence, and time from verified server context. Clients cannot forge these fields; attempts are refused instead of silently overwritten. State is an assertion, not evidence of correct completion or permission to mutate.

`lib/workload-compose.js:71` rejects receipts from ended sessions, other launches, malformed timestamps, expired windows, intervening wrap starts, and relevant control events. `:119` onward combines assertion, control, and observed activity. Availability requires a current complete/safe receipt plus observed engine at-rest (`:134`). Idle alone is not available, and unknown is not safe-to-clear.

**Spindle contract:** separate `ObservedActivity`, `WorkloadReceipt`, `ControlState`, and computed `Availability` types. Preserve provenance, freshness reason, and launch identity in API responses. Never infer safe termination from an idle terminal.

## 4. Messaging needs explicit uncertainty

`lib/medusa-exchanges.js:4` maintains append-only facts and a derived exchange projection. Facts may arrive out of order; the implementation ranks facts rather than assuming sequential delivery. Terminal outcomes compete in a transaction, and late facts remain auditable without reopening the exchange.

A send intent is committed before contacting the external Hub, then its returned ID is bound in a second transaction (`:20`, `:592`). A crash between them cannot be atomically solved by local SQLite. Unknown sends remain `send_unknown`; this module does not automatically retry them. Correlation uses Hub IDs, and arrivals without local intent become explicitly untracked records that can later be adopted.

The listener's inbox is in memory (`lib/medusa-listener.js:10`, `:264`), but this does not mean all mail is simply lost on restart. The source describes Hub unacknowledged delivery and acknowledgment tracking (`:283` onward). There are distinct read, acknowledgment, reply, and terminal facts. Opening a dashboard inbox must not impersonate an agent read/ack (`lib/medusa-exchanges.js:760`, `:789`). Acknowledging a message requiring a reply does not discharge the reply obligation (`:827`).

Control and escalation notices are not ordinary exchanges (`lib/medusa-exchanges.js:27`). Closing or retracting mail must never release a hold or change authority. Priority changes timing and visibility, not privileges.

**Spindle contract:** persist intent and external identity; expose accepted, delivered, read, acknowledged, replied, and unknown outcomes distinctly. Introduce retries only with a verified remote idempotency contract. Bound memory/backpressure, but report overflow and preserve durable obligations.

## 5. Handoff publication spans two failure domains

`lib/handoff-lockfile.js:14` explicitly recognizes that filesystem renames cannot join SQL transactions. Staging writes exact serialized bytes, computes their digest, and renames a temporary file (`:106`). Promotion retires the old publication before installing the new one (`:120` onward). This preserves historical evidence but creates crash windows that require reconciliation.

The inspected staging function does not call file or directory fsync. Its atomic rename prevents partial-file observation under ordinary execution; this alone is not proof of power-loss durability. Define and test the stronger guarantee if Spindle claims it.

Finalization is bound to an exact publication ID (`lib/handoff-publish.js:4`), checks eligibility while serialized, and treats already-published attempts as retryable success (`:57`). It distinguishes absent files from unreadable files (`lib/handoff-lockfile.js:76`); permission failure must not look like a first launch. Resume text and top-level next action are validated as the same value (`lib/handoff-publication.js:151`).

**Spindle contract:** durable publication intent, exact-byte digest, explicit staged/published/superseded states, startup reconciliation of file/database mismatches, and deterministic retry. Store continuity payloads transactionally where possible; export human-readable files as recoverable projections rather than duplicate authorities.

## 6. Lifecycle handoff is not ordinary session restart

`lib/lifecycle-handoff.js:18` defines checkpoint, delegation, graceful wrap, settled inactivity, one replacement, normal startup, reconciliation, takeover, and atomic closure. Important distinctions:

- A delegate cannot prove successor identity by copying a digest returned by an API (`:62`). Reconciliation requires the actual replacement caller and an attested ready launch.
- The terminal check reads recorded lifecycle status, not pane idleness (`:53`).
- Delegated authority is bound to the original generation; later actions must refuse when that generation changes (`:38`).
- Blocked handoffs preserve the checkpoint (`:76`).
- The daemon may drive evidence-backed progress, but cannot fabricate replacement reconciliation or takeover (`:81`).
- Additive schema and an open-handoff uniqueness fence participate in launch serialization. An external process launch still needs explicit crash/restart reconciliation; this review does not establish exactly-once spawning across all crash points.

**Spindle contract:** persist each transition, bind delegated authority to its generation and actor, reconcile successor process identity after crashes, and require authenticated replacement acknowledgment. Do not reduce this to "old process died, start another."

## 7. Rules in force, rendered rules, and delivered rules differ

`lib/session-rules-refresh.js:4` treats SQLite rows as authority and files as rendering. Refreshing files is not proof of delivery and writes no delivery receipt. Existing engine hooks may grow when a rule set grows, but opt-out and missing engine capability remain respected (`:18`). A new hook reaches context only when the engine triggers it, not immediately mid-turn (`:40`).

`lib/session-rules-channel.js:483` packs rules into fixed delivery slots and names omitted rules. An empty rules set has explicit rendering (`:548`) because a missing block can mean delivery failure. Preserve these distinctions rather than reporting all stored rules as context delivered.

**Spindle contract:** version rule sets, record rendered version and attested delivered version separately, expose omissions, and model capabilities by engine. Live refresh must not silently re-enable a channel the operator disabled.

## 8. Wrap is a recoverable workflow, not a shell script

`lib/wrap-pipeline.js:4` stages server-produced mutations until commit, stops on blocking outcomes, and declares explicit statuses (`:60`): unavailable capability and not-applicable are not success. Missing methodology capability withholds merge/release authority (`:72`) while still permitting checkpointing.

Retries reuse eligible captured content only for the same session, a blocked uncommitted run, and a bounded capture age (`:87`, 30 minutes). Retrying does not make an old capture fresh. All other step checks run again.

Ordering in `lib/wrap-default-pipeline.js` is semantic: ownership decisions precede writes; changelog fragments precede release preparation; commit precedes publication-dependent continuity. Flattening the workflow into loosely concurrent tasks would violate those dependencies.

`lib/wrap-steps/commit.js:6` stages files but does not provide a single transaction covering filesystem, SQLite, Git, and remote hosting. Commit failure may leave worktree edits for retry; uncertain timeout is not proof that no commit landed (`:38`). Hooks are respected (`:80`). Path ownership is checked before commit (`:87`), and unrelated changes need explicit decisions. Local commit, remote push, PR creation, merge, and continuity publication must remain separate outcomes.

**Spindle contract:** persist workflow run/step IDs, inputs and outputs, capability and authority verdicts, external operation receipts, and unresolved outcomes. Use exact path sets, argv subprocess invocation, cancellation deadlines, and post-timeout reconciliation. Never blindly repeat a possibly successful commit/push/merge.

## Acceptance cases for the rebuild

| Scenario | Required result |
|---|---|
| HOLD commits, notification transport fails, service restarts | Mutation remains refused; pending notification is visible/retryable |
| New HOLD races a RELEASE constructed earlier | Stale RELEASE fails; new hold survives |
| STOP followed by dropped listener or exchange closure | Assignment remains stopped |
| Old launch submits complete/safe workload | Refused or explicitly stale; cannot make current lane available |
| Engine is idle without current workload assertion | Unknown availability, not permission to clear |
| Send accepted remotely, process crashes before binding response | Unknown outcome surfaced; no blind duplicate send |
| Read/ack/reply facts arrive out of order or twice | Stable projection; no duplicate once-only effect |
| Dashboard opens inbox | No impersonated agent acknowledgment |
| Crash at each file rename/DB publication boundary | Deterministic reconciliation preserving evidence and prior publication |
| Handoff file is unreadable instead of absent | Typed recovery blocker, not empty-state initialization |
| Two delegates race or server restarts during successor launch | One authorized successor or explicit uncertainty requiring reconciliation |
| Non-replacement copies checkpoint digest | Cannot attest reconciliation/takeover |
| Rule retired during live session with hooks disabled | Stored truth updates; delivery status stays honest; opt-out preserved |
| Wrap times out during commit | Inspect actual Git state before retry; do not restore over possible commit |
| Unrelated staged/unstaged files exist | Preserved; excluded unless explicitly authorized |
| Control changes between commit and push | Local commit retained; push withheld |
| Schema import sees missing trigger or orphaned reference | Report/refuse safely; no silent weakening of constraints |

## Suggested implementation sequence

1. Implement database ownership, typed IDs, launch identity, and durable operation receipts in the session-lifecycle slice.
2. Add control transactions and a minimal outbox before autonomous workflow mutations.
3. Implement workload observation/composition with explicit stale and unknown states.
4. Add handoff publication/reconciliation and restart fault tests before automatic replacement sessions.
5. Add exchange facts and external send uncertainty, then rule delivery receipts.
6. Port wrap as a persisted dependency-ordered workflow only after Git ownership and uncertain-outcome contracts are tested.

These contracts are recommendations for Spindle, not claims that they have been implemented or validated there.

## Implementation follow-up

The foundation observations above describe the initial review snapshot. Before the first PR, Spindle added forward-schema refusal, indexed create/update lookups, legal transitions, bounded list replies, shutdown admission fencing and original-process-group cleanup. See [validation.md](validation.md) for executed evidence and remaining limits. These fixes do not satisfy the complete upstream parity requirements.
