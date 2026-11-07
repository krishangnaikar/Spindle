# API, browser, platform, integration, and delivery review

Snapshot: TangleClaw `3162811af43714b6d83ef4a43e6602adaead2785`, v5.71.0. Review date: 2026-11-07 America/Los_Angeles.

## Scope and evidence boundary

This is a static review and migration contract, not a security certification or platform execution report. Source searches, module inspection, feature-index cross-checks, and tracked-path enumeration were performed. No server, SSH connection, installer, engine, or test suite was executed for this review. The official marketing website and linked roadmap were read in a browser as documented below. Earlier measured audit results are separate evidence and are not newly reproduced here.

Every source reference below is repository-relative to this pinned snapshot. Line numbers are anchors, not a claim that an entire file was audited statement by statement. The inventory appendix exhaustively enumerates the selected directories and explicitly selected security/platform/integration modules. Other reviews own engine/session internals, storage, orchestration, and governance. Cross-boundary routes are included here for transport/authorization contracts only.

## Executive assessment

Spindle should retain the browser client while replacing backend execution and process ownership with explicit Rust services. The existing application has accumulated important correctness contracts that a fresh HTTP CRUD implementation would miss: HTTP and WebSocket authorization parity; signed-in operator proof distinct from machine access; canonical-path matching; safe terminal reattachment; stale response rejection; resumable operational workflows; and truthful unknown/degraded states.

A native Windows backend is new engineering. Browser-side Windows clipboard support is not evidence of Windows service, PTY, process-tree, filesystem, or engine support. Linux support is partly implemented, despite the primary installer still refusing non-macOS hosts. Full parity must include remote gateways, uploads, project/group/document management, status visibility, authentication recovery, release/update behavior, and mobile terminal usability, not only session spawning.

## Source-grounded contracts and acceptance gates

| Surface and source anchor | Existing contract to preserve | Spindle acceptance test |
|---|---|---|
| Router: `lib/routes/_kernel.js:20`, `:43`, `:67`, `:162` | Ordered method/pattern registration; decoded route parameters; structured error envelope; default 10 KiB JSON limit and 64 KiB message limit, with route overrides. Oversize responses distinguish exact declared lengths from lower bounds. | Replay an endpoint/verb catalog; malformed encoding never escapes the request boundary; test exact-limit, over-limit, chunked, invalid JSON, and abandoned connections. Preserve machine-readable error codes or version the API. |
| HTTP dispatch: `server.js:7983`, `:8141`, `:8254`, `:8295` | Cross-site write guards, session CSRF validation, launch-binding judgment, and optional signed-in operator proof are separate layers. A valid machine client is not automatically an operator approval. | Matrix across signed-out/browser cookie/machine token/session binding/operator, all mutation methods, forged proxy headers, and stale bindings. Authentication success must not skip authorization. |
| Auth policy: `lib/auth-gate.js:35`, `:152`, `:545`, `:657` | Open/account-required/armed/locked/unreadable/fallback states. Unreadable policy enforces; fallback requires proof of ingress protection. Canonical public bypasses are health and manifest; login/setup/recovery exemptions differ by state. | Table-driven state/path/method tests, including percent-encoded and normalized aliases; unreadable store/config cannot silently open the service. First-user setup cannot remain available after account creation. |
| Machine trust: `lib/auth-gate.js:433`, `:545`; `lib/service-token.js:135` | Local machine classification depends on loopback, absence of proxy/browser shape and session cookies, with separately controlled service-token policy. This is an existing trust choice, not proof every local caller has operator authority. | Decide and document Spindle's local trust model; default token policy must be explicit. A proxied loopback socket is never classified as an originating local caller. Preserve agent binding independently. |
| Session credentials: `lib/auth-session.js:59`, `:76`, `:168`, `:200`, `:267`, `:306`; `lib/password.js:57`, `:118` | Random session credentials, hashed storage representation, HttpOnly session cookie, separate CSRF cookie, SameSite policy, proxy cookie stripping and constant-time CSRF comparison. Password work has async entry points. | Expiry, revocation, recovery-code single use, logout CSRF, concurrent recovery, session rotation, and upstream proxy redaction. Password hashing must not block Tokio workers. |
| WebSocket perimeter: `server.js:7595`, `:7644`, `:7689`, `:7823`; `lib/auth-gate.js:737` | Upgrade handling is independent of ordinary HTTP dispatch. Origin/served-host/auth checks precede terminal and gateway branches. App cookies are removed from terminal proxy requests. | Unauthenticated or cross-origin terminal/gateway upgrades fail even when HTTP routes pass. Malformed upgrade targets close cleanly; backpressure and disconnect cleanup are bounded. |
| Gateway proxy: `lib/routes/openclaw.js:186`, `:226`, `:254` | Stored gateway credentials are injected server-side; attribution headers are sanitized. Proxy access therefore grants authority and must remain behind the perimeter. | Canary credentials must never reach client JSON/logs/other origins; HTTP and WS forwarding use identical policy. Test denied connection, gateway restart and pending device pairing separately. |
| Gateway lifecycle: `lib/routes/openclaw.js:323`, `:419`, `:533`, `:627`, `:717`, `:852`; `lib/openclaw-remote.js:179`, `:236` | Connection creation, discovery, address migration, SSH/health testing, tunnels and approval are distinct operations. Host resolution reports actual reachable candidates, with route invalidation. | Name/address changes, DNS failure, black-holed SSH, partial tunnel startup, stale port leases, tunnel teardown, and unavailable gateway report specific states; do not equate open TCP with authenticated service health. |
| Dashboard: `public/landing.js:3283`; `public/ui.js:431`, `:1496`, `:1768` | Projects refresh every 10 seconds and multiple panels every 30 seconds via timeout chains. Peek generations and focus restoration protect asynchronous UI updates. | Delayed earlier responses cannot overwrite newer selections. Keyboard focus survives refresh. Multi-tab behavior is measured. Stream-based updates retain snapshot reconciliation after reconnect. |
| Session UI: `public/session.js:24`, `:1655`, `:2056`, `:2403`, `:6639` | Default 5-second status refresh, chained polling, bounded terminal reconnect, ended-session monitoring, and wrap stream/poll fallback. Unreadable iframe alone is not confirmed terminal failure. | Restart server without killing session, reconnect after terminal restart, stopped session stays stopped, and inaccessible frame does not start an infinite reattach loop. Test slow polling and old-generation responses. |
| Reconnect: `public/reconnect-policy.js:9`, `:61` | Elapsed-time outage ceiling (20 s), nominal 5 s jittered probes, generation cancellation, independent ceiling timer for stalled requests. Background-tab throttling cannot be modeled as fixed attempt count. | Fake-clock foreground/background tests, black-holed requests, connected/disconnected flaps, one active retry chain, recovery clears warning. |
| Terminal usability: `public/api-helper.js:939`; `public/session.html`; `public/session.css`; `FEATURES.md:297` | Touch keys, sticky control, selection/copy, Windows browser paste, mobile keyboard viewport sizing, link tap behavior, full screen/pop-out. These are product features, not styling extras. | Real Chromium/Firefox/WebKit browser runs plus mobile keyboard and Windows clipboard checks. Avoid relying solely on DOM stubs. Pop-out attaches the same session. |
| PWA delivery: `public/sw.js:14`, `:351`, `:373`, `:404`; `public/sw-register.js` | Static precache plus network-first API/navigation/critical assets; successful GET responses only are cached, streaming responses excluded. Cache fallback is marked. Worker upgrade propagation must avoid mixed script generations. | Upgrade N to N+1 with an existing worker, offline shell load, expired auth, missing JS, stream response, cache write failure, iOS/background resume. Cached status cannot masquerade as fresh operational authority. |
| Feature mounting: `public/sw.js:67`; `FEATURES.md` | Presence or precaching of a helper does not prove it is called. Snapshot comments explicitly distinguish mounted scripts from uncalled governed-wrap/chunk-boundary functions. | Parity ledger labels reachable, helper-only, experimental, deprecated. Require a user-flow trace before claiming a feature complete. |
| Uploads: `lib/uploads.js:82`, `:147`; `public/upload-pills.js`; `public/upload-pills-ui.js` | Project-scoped file listing/saving and session association are separate from rendering attachment pills. | Path traversal, symlink escape, filename collisions, oversized base64, partial writes, orphan cleanup, and wrong-project access. Set explicit memory limits before decoding. |
| Audit API: `lib/routes/audit.js:99`, `:406`, `:479`, `:653`, `:745` | Ingest/heartbeat, telemetry, scores, anomalies, baselines, incidents, human scores and retention are distinct feature families. | Invalid event schema, stale session attribution, retention bounds, unavailable probe and score provenance. Unknown must not appear as a healthy score. |
| Restart status: `lib/server-info.js:292`, `:368`, `:408`, `:435` | Restart capability is probed, then rechecked at invocation. Linux systemd ownership and macOS launchd ownership determine the action; unsupported/manual ownership does not gain a button. | Process-manager owner changes between display and click cause refusal. Running version and installed-on-disk version remain distinguishable. |
| Linux service: `lib/linux-systemd.js:91`, `:122`, `:166`, `:216` | Direct Node ExecStart, process ownership, restart contract, user unit, linger and step-specific install results. | Rust executable runs directly; API restart preserves terminal host as designed; unit quoting handles spaces; install/uninstall are repeatable; user-manager unavailable is actionable. Physical host validation remains required. |
| macOS install: `deploy/install.sh:22`, `:106`, `:139`; `deploy/*.plist` | Main installer checks Darwin before writes. Homebrew dependencies, direct/Caddy ingress distinction and preflight prevent predictable destructive partial installation. | Fresh/upgrade/uninstall on macOS; existing customized ingress not overwritten; failed dependency/CA provisioning reports exact partial state. Preserve user configuration. |
| Ingress and CA: `lib/caddy.js:213`, `:422`, `:615`, `:709`, `:1300`; `lib/remote-ca-installer.js:49`, `:81` | Canonical paths, generated configuration ownership, certificate staging and credential adoption carry security consequences. Remote CA steps are planned/executed explicitly. | Misconfigured certificate or listener must not silently expose the expected secure service; unmanaged ingress requires reconciliation. Test off-box access and rejected hostnames separately. |
| Update/install identity: `lib/update-applier.js:368`, `:649`, `:718`, `:769`, `:975` | Existing updater manipulates a source checkout, classifies edits, protects carried content and backs up DB. This cannot simply be renamed a binary updater. | Signed/versioned artifact policy, atomic replacement, failed restart rollback, schema compatibility, retained user files, disk-full and interrupted update tests. Do not conflate release availability, disk version, and running process. |
| Test tiers: `scripts/test-tiers.js:5`; `test/helpers/preload-small.js`; `test/helpers/preload-medium.js` | Small forbids subprocess/database/socket; medium permits temp DB/HTTP but forbids subprocess; large permits external integration. Runtime guards complement static classification. | Separate pure state tests, isolated storage/API tests, real PTY tests and device/platform acceptance. A passing Linux unit job cannot certify macOS/Windows or actual engines. |
| CI/release: `.github/workflows/test.yml:34`, `:44`, `:94`; `scripts/cache-bump-guard.js`; `scripts/test-skip-audit.js` | Read-only contents token, checkout without persisted credentials, job timeout, suite/records distinction, skip visibility, cache-diff gate; release workflow reuses validation. | Rust format/lint/tests on PRs, dependency/reproducibility checks, OS matrix, explicit integration skips, artifact provenance and required checks. Never execute contributor code with publishing secrets. |

## Remaining product surface checklist

The shared HTTP entry point still contains many route families in addition to extracted `lib/routes` modules. Endpoint migration must start from the runtime-registered route catalog, not only a text search for literal `route(...)`. The earlier audit found 403 registered routes versus 269 static literal declarations; this review does not re-execute that count.

- Project discovery, create/import/detach/archive/delete, filters/tags, settings, engine selection, generated-document status and safe launch intent.
- Global and project rules, session rule receipts/promotions, learning tiers, startup prompts and signed-in approval paths.
- Groups, members, shared documents, document watching, access rules and external repository freshness.
- Terminal/session control, master control panel, session history, ended-state relaunch, uploads and touch controls.
- Wrap streams, polling fallback, handback, chunk boundary, fleet recovery, sprint waves and operator bridge settings.
- OpenClaw connection/tunnel/SSH/address migration/embedded UI; remote CA onboarding and device approval.
- Audit ingest, telemetry, incidents, retention, score explanations, usage windows and health/recovery panels.
- Config/setup, account setup/login/logout/recovery, install-token handling, service credentials, ingress drift and runtime diagnostics.
- CLI `bin/tc`, bridge helper/receiver, hooked engine callbacks, command exit codes and actionable structured errors.
- Source-checkout/update/release visibility, binary distribution replacement, docs/reference parity, release evidence and marketing site.

`website/` is a separate Next/React marketing site, not the production browser frontend in `public/`. Its package manifest is `website/package.json:1`; application code is under `website/src/app`. Rebuilding the orchestration service does not require migrating this website to Rust. Marketing assets/screenshots and telemetry should be handled independently from local application behavior.

## Rust design consequences

1. Define typed request/response/error and event schemas, plus an endpoint parity ledger. Represent identity, session binding and approval separately; avoid a single boolean `authenticated` shared across all operations.
2. Keep HTTP/WS middleware policy in one Rust library used by both transports. Apply bounded JSON/upload/terminal buffers before allocating unbounded data. Carry cancellation and deadlines through remote operations.
3. Isolate the terminal host and own its lifetime explicitly. Linux systemd, macOS launchd and Windows services/process jobs need separate integration tests. A ConPTY spike must prove resize, Unicode, paste, output drainage, disconnect and process-tree shutdown before promising Windows parity.
4. Share observed snapshots and event subscriptions to reduce per-tab probes. Events must have revisions and resumable cursors; reconnect takes a consistent snapshot and deduplicates events. Preserve unknown/error states.
5. Keep gateway/SSH/CA work outside request-critical executors and behind explicit capabilities. Redact at the boundary, not only at the logger. Enforce output and concurrency limits.
6. Publish versioned frontend assets with an atomic manifest. Preserve accessibility and touch behavior while decomposing the large global-script files into TypeScript modules.
7. Treat installer/updater/rollback as their own product. Porting source-checkout update mechanics into a Rust binary is not a safe migration strategy.

## Priority implementation slices

- Foundation: health/version, typed errors, loopback bind and explicit auth policy, migrations, structured redacted logs and bounded shutdown.
- Session slice: launch/attach/resize/stop, reconnect, output backpressure and API restart survival. Test Linux live and Windows terminal host early.
- Browser slice: project/session list, truthful state, terminal controls, mobile behavior and version-coherent assets.
- Security slice: complete identity/CSRF/upgrade/operator/binding matrix before remote ingress or gateway proxy enablement.
- Integration slice: gateway management, tunnels, groups/docs/uploads and CLI compatibility with contract fixtures.
- Full parity slice: remaining audit, control/recovery, workflow and update surfaces with explicit per-feature acceptance status.

## Official website and public roadmap review

Accessed 2026-11-07 America/Los_Angeles. The official URL is verified from pinned `README.md:58` and `website/src/app/layout.tsx:43`: [tangleclaw.com](https://tangleclaw.com/). The roadmap URL is linked at `README.md:15`: [public roadmap](https://jason-vaughan.github.io/TangleClaw/). Web extraction returned `Internal Error` and reported both URLs inaccessible through that tool. Browser fallback successfully displayed both pages. This is an access-path failure, not evidence that the websites were down.

The homepage displayed v5.71.0, matching the pinned source. Its primary journey is install/view repository, followed by feature explanations and interface screenshots. It presents persistent local orchestration, visibility across sessions, project leadership, continuity, engine coordination, governance, GitHub delivery and local security. Gallery captions cover dashboard, master, settings, gateways, history, rules and audit. Scale and expanded coordination are separately presented as roadmap subjects. The page does not offer an interactive application demo. [Observed homepage](https://tangleclaw.com/).

| Website framing | Source-grounded interpretation and Spindle implication |
|---|---|
| Persistent work across network interruption | `lib/tmux.js` and the terminal proxy support the product direction, but network detach, API restart, terminal-host death and machine reboot are different guarantees. Spindle should publish and test each separately. |
| Local-first operation | The service and state live locally, but selected engine providers and optional remote gateways can use external services. Do not market this as proof of offline operation or no data egress. |
| Desktop/mobile access | Browser viewport, keybar, copy/paste and reconnect code support this goal. Real-device acceptance is still necessary; screenshot presence is not execution evidence. |
| Engine coordination | Profiles express capability differences. A product-level list of engines must not imply identical launch, native protocol, context, recovery or permissions behavior. |
| Authentication and secure remote operation | The current auth/session/upgrade/ingress modules substantiate implemented controls. Configuration and trust-boundary details still matter; do not reduce these contracts to a TLS checkbox. |
| Portable fleets in future scope | Native Windows service/PTY support is not established by this homepage or by Windows browser paste handling. Retain it as a new Spindle acceptance target. |

The roadmap displayed a GitHub snapshot timestamp of **2026-11-06 09:30:00 PST**, despite being read the following day; it must not be described as a live API view. It explicitly calls plans nonbinding and directs readers to tags/changelog for shipped contents. v5.73.0 included Linux systemd verification and Medusa wake work still awaiting checks; v6 listed multi-user/container/fleet architecture as planned. v5.72.0 listed hierarchy, parallelization authorization, confirmed engine failover and other future work. [Observed roadmap](https://jason-vaughan.github.io/TangleClaw/).

The strongest discrepancy is one of **scope**, rather than a demonstrably false homepage statement: broad capability summaries hide OS maturity and per-engine differences. The README's macOS prerequisites and installer guard, partly implemented Linux support, deferred physical validation, and roadmap-only multi-user architecture must remain visible in Spindle's parity ledger. A full TangleClaw parity target should mean shipped, reachable behavior at the pinned revision; roadmap items are separately prioritized additions. The website itself already separates future ambitions from current feature sections.

## Inventory methodology

The appendix lists tracked paths, grouped by selected directory; files are not claimed as individually executed or fully line-reviewed. For core `lib` it lists the explicit modules selected for this boundary review. Tests are inventoried as a whole because behavior often spans these boundaries; their presence does not imply a passing run. Non-text binary assets are inventoried only. Documentation contains historical/proposed work and is evidence to cross-check, not an automatic live-feature claim.

### public/ (53 tracked paths)

<details>
<summary>Tracked inventory</summary>

```text
public/account-setup.html
public/api-helper.js
public/beacon.css
public/bridge-settings-ui.js
public/bridge-step-up.js
public/chunk-boundary-ui.js
public/fleet-recovery-panel.js
public/governed-wrap-ui.js
public/health-panel.js
public/history-drawer.js
public/icons/apple-touch-icon.png
public/icons/icon-192.png
public/icons/icon-512.png
public/index.html
public/landing.js
public/login.html
public/logo-banner.png
public/logo-icon.png
public/logo-text.png
public/logo.png
public/manifest.json
public/medusa-head-left.webp
public/medusa-head-right.webp
public/medusa-loops-ui.js
public/medusa-wordmark.webp
public/next-markdown.js
public/openclaw-cache.js
public/openclaw-tunnel-state.js
public/openclaw-view.html
public/openclaw-view.js
public/operator-bridge-panel.js
public/reconnect-policy.js
public/recover.html
public/remote-ca-assist.js
public/session-history-ui.js
public/session-relaunch.js
public/session.css
public/session.html
public/session.js
public/setup.js
public/shared-controls.css
public/sprint-wave-ui.js
public/style.css
public/sw-register.js
public/sw.js
public/terminal-identity-ui.js
public/ui.js
public/update-beacon.js
public/upload-pills-ui.js
public/upload-pills.js
public/wrap-drawer.js
public/wrap-run-controller.js
public/wrap-stream-events.js
```

</details>

### lib/routes/ (12 tracked paths)

<details>
<summary>Tracked inventory</summary>

```text
lib/routes/_callers.js
lib/routes/_kernel.js
lib/routes/audit.js
lib/routes/config.js
lib/routes/engines-canary.js
lib/routes/launch-recovery.js
lib/routes/openclaw.js
lib/routes/sessions.js
lib/routes/setup.js
lib/routes/sprint-wave-routes.js
lib/routes/tc.js
lib/routes/wrap.js
```

</details>

### deploy/ (25 tracked paths)

<details>
<summary>Tracked inventory</summary>

```text
deploy/INGRESS.md
deploy/VRF-1901-caddy-asset-refresh.md
deploy/VRF-auth-1-cutover.md
deploy/cleanroom/README.md
deploy/cleanroom/bake.sh
deploy/cleanroom/compose.yaml
deploy/cleanroom/provision.sh
deploy/com.tangleclaw.bridge-helper.plist
deploy/com.tangleclaw.caddy.plist
deploy/com.tangleclaw.server.plist
deploy/com.tangleclaw.ttyd.plist
deploy/install.sh
deploy/soak/README.md
deploy/soak/guest/guest-setup.sh
deploy/soak/guest/guest.conf
deploy/soak/guest/host-provision.sh
deploy/soak/guest/pf/soak-deny.conf
deploy/soak/medusa-stub/medusa-stub.js
deploy/soak/stub-engine/soak-stub.js
deploy/soak/stub-engine/soak-stub.json
deploy/tmux.conf
deploy/ttyd-attach.sh
deploy/ttyd/inputs.json
deploy/ttyd/patches/0001-pr1573-keep-reading-after-zero-byte-read.diff
deploy/ttyd/patches/0002-darwin-drain-after-close.diff
```

</details>

### scripts/ (55 tracked paths)

<details>
<summary>Tracked inventory</summary>

```text
scripts/_require-graph.js
scripts/apply-update.js
scripts/assert-strict-guard.js
scripts/audit-dead-exports.js
scripts/audit-source-readers.js
scripts/audit-test-redundancy.js
scripts/build-ttyd.js
scripts/cache-bump-guard.js
scripts/cc7-capture-spike.js
scripts/changelog-fragment-gate.js
scripts/check-bump-diff.js
scripts/ci-complement.js
scripts/ci-evidence.js
scripts/conflict-marker-scan.js
scripts/docs-parity-gate.js
scripts/docs-records-only-diff.js
scripts/drill-gate-fallback.js
scripts/gate-fallback.js
scripts/guard-primary-checkout.js
scripts/guard-ungated-sites.js
scripts/host-check.js
scripts/ingress-cutover.js
scripts/install-primary-guard.js
scripts/linux-systemd-install.js
scripts/medusa-wake-matrix.js
scripts/pin-https-listener.js
scripts/pre-push-gate.js
scripts/rc-cert.js
scripts/record-test-run.js
scripts/regen-caddy-adapt-fixtures.js
scripts/release-notes-gate.js
scripts/release-prepare.js
scripts/release-tag-gate.js
scripts/reorganize_roadmap.py
scripts/repair-governed-claude-md.js
scripts/reset-admin.js
scripts/retire-stale-worktrees.js
scripts/roadmap-sync-check.js
scripts/rotation-live-check.js
scripts/scorecard-produce.js
scripts/scorecard-publish-metrics.js
scripts/scorecard-verify.js
scripts/soak.js
scripts/suite-evidence.js
scripts/sync-claude-md-block.js
scripts/test-skip-audit.js
scripts/test-stage.js
scripts/test-tiers.js
scripts/test-touched.js
scripts/ttyd-churn.js
scripts/ttyd-runtime.js
scripts/vercel-ignore-gh-pages.sh
scripts/verify-features-index.js
scripts/verify-update-beacon-restart.js
scripts/with-scratch-server.js
```

</details>

### bin/ (3 tracked paths)

<details>
<summary>Tracked inventory</summary>

```text
bin/tc
bin/tc-bridge-helper
bin/tc-bridge-receive
```

</details>

### .github/ (7 tracked paths)

<details>
<summary>Tracked inventory</summary>

```text
.github/ISSUE_TEMPLATE/bug_report.md
.github/ISSUE_TEMPLATE/feature_request.md
.github/dependabot.yml
.github/workflows/release.yml
.github/workflows/scorecard-verify.yml
.github/workflows/test.yml
.github/workflows/upstream-drift.yml
```

</details>

### website/ (37 tracked paths)

<details>
<summary>Tracked inventory</summary>

```text
website/.gitignore
website/AGENTS.md
website/CLAUDE.md
website/README.md
website/docs/design-specs.md
website/eslint.config.mjs
website/next.config.ts
website/package-lock.json
website/package.json
website/postcss.config.mjs
website/public/favicon.ico
website/public/file.svg
website/public/globe.svg
website/public/logo-text.png
website/public/next.svg
website/public/screenshots/Dashboard.png
website/public/screenshots/Project Rules 2.png
website/public/screenshots/eval audit.png
website/public/screenshots/global settings.png
website/public/screenshots/master session.png
website/public/screenshots/open claw integration.png
website/public/screenshots/open claw list.png
website/public/screenshots/project settings 1.png
website/public/screenshots/project settings 2 - startup and wrap rules.png
website/public/screenshots/session history detail.png
website/public/screenshots/session history summary list.png
website/public/tangleclaw-logo-real.png
website/public/vercel.svg
website/public/window.svg
website/src/app/components/VersionBadge.tsx
website/src/app/favicon.ico
website/src/app/globals.css
website/src/app/icon.png
website/src/app/layout.tsx
website/src/app/page.tsx
website/src/app/providers.tsx
website/tsconfig.json
```

</details>

### hooks/ (3 tracked paths)

<details>
<summary>Tracked inventory</summary>

```text
hooks/commit-msg
hooks/post-commit
hooks/pre-commit
```

</details>

### test/ (937 tracked paths)

<details>
<summary>Tracked inventory</summary>

```text
test/_api-helper-globals.js
test/_bridge-hub.js
test/_caddy-drift-fixtures.js
test/_caddy-stub.js
test/_codex-launchable.js
test/_codex-startup-fixtures.js
test/_codex-visible-pane-fixtures.js
test/_dir-scanner-hang-child.js
test/_dir-scanner-pool-demo.js
test/_dir-scanner-stderr-child.js
test/_eacces.js
test/_engine-fixture.js
test/_engine-store.js
test/_exec-results.js
test/_fake-discord.js
test/_master-liveness.js
test/_mini-dom.js
test/_page-globals.js
test/_prime-golden-scenarios.js
test/_probe-stub.js
test/_recovery-fixture.js
test/_release-certification-fixtures.js
test/_shared-docs-callers.js
test/_tc-env.js
test/_temp-repo.js
test/_tmux-guard.js
test/_tmux-session-names.js
test/_wake-fixtures.js
test/_wrap-scope-fixture.js
test/account-self-service-ui.test.js
test/account-setup-page.test.js
test/actions-dispatcher.test.js
test/actions-invoke-critic.test.js
test/activity-observer.test.js
test/admin-credential.test.js
test/advisor-control-bar.test.js
test/advisor.test.js
test/advisory-ready-audit.test.js
test/aider-engine.test.js
test/aider-task-contract.test.js
test/antigravity-engine.test.js
test/api-account-self-service.test.js
test/api-actions.test.js
test/api-add-login.test.js
test/api-audit.test.js
test/api-auth-identity.test.js
test/api-auth-session.test.js
test/api-bridge-gateway.test.js
test/api-bridge.test.js
test/api-chunk-boundary.test.js
test/api-config-default-engine.test.js
test/api-config-tailscale.test.js
test/api-config.test.js
test/api-continuity.test.js
test/api-control.test.js
test/api-coordinator-rotation.test.js
test/api-decisions.test.js
test/api-gate-fallback.test.js
test/api-globalrules.test.js
test/api-governed-wrap.test.js
test/api-groups.test.js
test/api-ingress-door.test.js
test/api-integration.test.js
test/api-launch-sequences.test.js
test/api-lifecycle-handoff.test.js
test/api-medusa-exchanges.test.js
test/api-medusa.test.js
test/api-openclaw.test.js
test/api-permissions.test.js
test/api-plan-docs.test.js
test/api-ports.test.js
test/api-projects.test.js
test/api-recovery-codes.test.js
test/api-scorecard.test.js
test/api-self-improvement.test.js
test/api-service-token.test.js
test/api-session-finalize.test.js
test/api-session-rules-authz.test.js
test/api-session-rules-selfimprove.test.js
test/api-session-rules.test.js
test/api-sessions.test.js
test/api-setup-https.test.js
test/api-shareddocs-read-enforcement.test.js
test/api-shareddocs-write-enforcement.test.js
test/api-shareddocs.test.js
test/api-stranded-wraps.test.js
test/api-system.test.js
test/api-update-apply.test.js
test/api-uploads.test.js
test/api-wrap-handback.test.js
test/api-wrap-pr-status.test.js
test/api-wrap-status.test.js
test/api-wrap-stream.test.js
test/areas-map.test.js
test/assert-strict-guard.test.js
test/audit-dead-exports.test.js
test/audit-source-readers.test.js
test/audit-test-redundancy.test.js
test/auth-credential-durability.test.js
test/auth-credential-endpoint.test.js
test/auth-gate-loopback.test.js
test/auth-gate.test.js
test/auth-identity.test.js
test/auth-session.test.js
test/auth-status-warning.test.js
test/auth2-setup-admin.test.js
test/auth2-wizard-admin.test.js
test/auto-merge-callers.test.js
test/awareness-observability.test.js
test/behind-origin.test.js
test/bind-notice-render.test.js
test/bind-policy-wiring.test.js
test/bind-policy.test.js
test/branch-ownership.test.js
test/branch-retire-safety.test.js
test/bridge-arrival-observer.test.js
test/bridge-awareness.test.js
test/bridge-candidates.test.js
test/bridge-channels.test.js
test/bridge-confirmation.test.js
test/bridge-context.test.js
test/bridge-dm-inbound.test.js
test/bridge-gateway.test.js
test/bridge-handoff.test.js
test/bridge-helper-advisory.test.js
test/bridge-helper-cli.test.js
test/bridge-helper-gateway.test.js
test/bridge-helper-relay.test.js
test/bridge-helper-units.test.js
test/bridge-leases.test.js
test/bridge-notify.test.js
test/bridge-outbound-decisions.test.js
test/bridge-port-input.test.js
test/bridge-principal.test.js
test/bridge-reach.test.js
test/bridge-reply-correlation.test.js
test/bridge-retention.test.js
test/bridge-settings-ui.test.js
test/bridge-slice2-wiring.test.js
test/bridge-status-timestamps.test.js
test/bridge-step-up-settings-wiring.test.js
test/bridge-store.test.js
test/build-ttyd.test.js
test/button-touch-targets.test.js
test/c1-plugin-migration.test.js
test/cache-bump-guard.test.js
test/caddy-drift-surfacing.test.js
test/caddy-drift.test.js
test/caddy-ingress-state.test.js
test/caddy.test.js
test/card-detail-disclosure.test.js
test/card-session-contract.test.js
test/change-log-merge.test.js
test/changelog-fragment-gate.test.js
test/changelog-fragments-repo.test.js
test/changelog-fragments.test.js
test/changelog-notes.test.js
test/changelog-released-immutable.test.js
test/changelog-structure.test.js
test/changelog-unreleased-size.test.js
test/check-bump-diff.test.js
test/check-watch-api.test.js
test/check-watch-monitor.test.js
test/check-watch-state.test.js
test/check-watch-store.test.js
test/checkout-fingerprint.test.js
test/checkout-fleet.test.js
test/checkout-freshness.test.js
test/checkout-layout.test.js
test/checkout-ownership.test.js
test/checkout-state.test.js
test/chime-at-prompt.test.js
test/chime-control.test.js
test/chime-status-payload.test.js
test/chunk-boundary-http.test.js
test/chunk-boundary-ui-browser.test.js
test/chunk-boundary.test.js
test/ci-complement.test.js
test/ci-evidence.test.js
test/ci-status.test.js
test/ci-workflow.test.js
test/claude-launch-env.test.js
test/claude-md-operational-block.test.js
test/clawbridge.test.js
test/cleanroom-compose.test.js
test/clipboard-copy.test.js
test/codex-launch-modes.test.js
test/codex-preflight.test.js
test/codex-readiness-timeout-paste.test.js
test/condition-log.test.js
test/conflict-marker-scan.test.js
test/continuity.test.js
test/contracts.test.js
test/control-auth.test.js
test/control-docs.test.js
test/control-e2e.test.js
test/control-gate.test.js
test/control-hold-prune.test.js
test/control-hooks-https.test.js
test/control-hooks-wiring.test.js
test/control-hooks.test.js
test/control-state.test.js
test/control-surfaces.test.js
test/coordinator-rotation-decider.test.js
test/coordinator-rotation.test.js
test/create-flow-mode-picker.test.js
test/create-launch-posture.test.js
test/create-project-modal.test.js
test/dashboard-boot-beacon.test.js
test/dashboard-filters.test.js
test/dead-exports-baseline.json
test/decision-routing.test.js
test/default-engine-wiring.test.js
test/degraded-reads-frontend.test.js
test/dependabot-config.test.js
test/dir-scanner-child.test.js
test/dir-scanner.test.js
test/docs-parity-gate.test.js
test/docs-records-only-diff.test.js
test/docs-w1a1-pointers.test.js
test/drill-gate-fallback.test.js
test/e2e-smoke.test.js
test/ecosystem-primer.test.js
test/engine-admission.test.js
test/engine-canary.test.js
test/engine-capability-reads.test.js
test/engine-change-launch-mode-ui.test.js
test/engine-config-managed-block.test.js
test/engine-error-monitor.test.js
test/engine-error-surface.test.js
test/engine-error-ui.test.js
test/engine-errors.test.js
test/engine-hooks-location.test.js
test/engine-hooks-merge.test.js
test/engine-launch-flags.test.js
test/engine-launch-modes.test.js
test/engine-models.test.js
test/engine-picker-gating.test.js
test/engine-profile-sync.test.js
test/engine-switch-retires-config.test.js
test/engine-tabs.test.js
test/engine-temp-root.test.js
test/engine-wake-profiles.test.js
test/engines-hook-shell-safety.test.js
test/engines.test.js
test/error-string-parity.test.js
test/esc-contract.test.js
test/escalation.test.js
test/eval-audit-reachable.test.js
test/eval-audit.test.js
test/exec-timeout.test.js
test/exec.test.js
test/extension-rule-values.test.js
test/feature-index-prime.test.js
test/feature-index.test.js
test/features-index.test.js
test/fixtures/antigravity-composer-m1-stranded-nudge.txt
test/fixtures/caddy-adapt-armed-catch-all.json
test/fixtures/caddy-adapt-armed-lan.json
test/fixtures/caddy-adapt-armed-public.json
test/fixtures/caddy-adapt-armed.json
test/fixtures/caddy-adapt-forwarded-for.json
test/fixtures/caddy-adapt-generated.json
test/fixtures/caddy-adapt-hand-edited.json
test/fixtures/caddy-adapt-handle-errors.json
test/fixtures/caddy-adapt-live-shape-gated.json
test/fixtures/caddy-adapt-live-shape-own-auth.json
test/fixtures/caddy-adapt-no-h1.json
test/fixtures/caddy-adapt-per-site-gate.json
test/fixtures/caddy-adapt-ungated-unguarded.json
test/fixtures/caddy-adapt-ungated.json
test/fixtures/changelog-released-sections.lock.json
test/fixtures/codex-panes/codex-0.156.1.json
test/fixtures/codex-panes/codex-0.159.0.json
test/fixtures/codex-panes/codex-0.161.0.json
test/fixtures/codex-panes/codex-0.162.0.json
test/fixtures/github-checks/commit-with-actions-run.json
test/fixtures/github-checks/commit-with-runs.json
test/fixtures/github-checks/commit-without-runs.json
test/fixtures/github-checks/named-check-projected.json
test/fixtures/github-checks/workflow-run.json
test/fixtures/host-check/README.md
test/fixtures/host-check/caddyfiles/per-site-certs.caddyfile
test/fixtures/host-check/caddyfiles/two-tls-sites.caddyfile
test/fixtures/host-check/caddyfiles/unsupported-import.caddyfile
test/fixtures/host-check/caddyfiles/v1-multihost.caddyfile
test/fixtures/host-check/certs.json
test/fixtures/host-check/spy-preload.js
test/fixtures/host-check/tailscale/serve-empty.json
test/fixtures/host-check/tailscale/serve-routes.json
test/fixtures/host-check/tailscale/status-magicdns-off.json
test/fixtures/host-check/tailscale/status-needslogin.json
test/fixtures/host-check/tailscale/status-no-name.json
test/fixtures/host-check/tailscale/status-running.json
test/fixtures/host-check/tailscale/status-stopped.json
test/fixtures/merge-queue-github/README.md
test/fixtures/merge-queue-github/candidate-ref-created.json
test/fixtures/merge-queue-github/candidate-ref-deleted.json
test/fixtures/merge-queue-github/checks-runs-candidate.json
test/fixtures/merge-queue-github/checks-runs-main-foreign.json
test/fixtures/merge-queue-github/checks-runs-rerun-all-PROBE.json
test/fixtures/merge-queue-github/checks-runs-rerun.json
test/fixtures/merge-queue-github/checks-suites-candidate.json
test/fixtures/merge-queue-github/checks-suites-main-foreign.json
test/fixtures/merge-queue-github/checks-suites-rerun.json
test/fixtures/merge-queue-github/commit-candidate.json
test/fixtures/merge-queue-github/commit-main-merge.json
test/fixtures/merge-queue-github/compare-candidate.json
test/fixtures/merge-queue-github/compare-diverged.json
test/fixtures/merge-queue-github/compare-identical.json
test/fixtures/merge-queue-github/compare-main-merge.json
test/fixtures/merge-queue-github/merge-conflict.json
test/fixtures/merge-queue-github/merge-nothing.json
test/fixtures/merge-queue-github/pull-files-page.json
test/fixtures/merge-queue-github/pull-merge-DOCUMENTED.json
test/fixtures/merge-queue-github/pull-merged.json
test/fixtures/merge-queue-github/pull-not-found.json
test/fixtures/merge-queue-github/pull-open-draft.json
test/fixtures/merge-queue-github/pull-open-with-merge-sha.json
test/fixtures/merge-queue-github/ref-candidate-gone.json
test/fixtures/merge-queue-github/ref-candidate.json
test/fixtures/merge-queue-github/ref-create-exists.json
test/fixtures/merge-queue-github/ref-delete-missing.json
test/fixtures/merge-queue-github/ref-deleted.json
test/fixtures/merge-queue-github/ref-main.json
test/fixtures/merge-queue-github/ref-not-found.json
test/fixtures/merge-queue-github/stage-merge-created.json
test/fixtures/merge-queue-github/stage-merge-read.json
test/fixtures/merge-queue-github/stage-ref-created.json
test/fixtures/merge-queue-github/stage-ref-delete-missing.json
test/fixtures/merge-queue-github/stage-ref-deleted.json
test/fixtures/openclaw-control-ui-2026.6.11.html
test/fixtures/openclaw-control-ui-2026.9.4.html
test/fixtures/prime-golden/audit-openclaw.txt
test/fixtures/prime-golden/fresh-antigravity.txt
test/fixtures/prime-golden/full-silent-claude-pull.txt
test/fixtures/prime-golden/full-silent-claude.txt
test/fixtures/prime-golden/overflow-claude.txt
test/fixtures/prime-golden/paste-codex-pull.txt
test/fixtures/prime-golden/paste-codex.txt
test/fixtures/scorecard-v1.json
test/fleet-provisioning.test.js
test/fleet-recovery-panel.test.js
test/fleet-settings-resource-routes.test.js
test/fleet-workload-view.test.js
test/frontend-api-errors.test.js
test/frontend-csrf.test.js
test/gate-fallback-command.test.js
test/gate-fallback.test.js
test/gh-issue-state.test.js
test/git-budget.test.js
test/git-hooks.test.js
test/git-probe.test.js
test/git-template.test.js
test/git.test.js
test/github-checks.test.js
test/github-facts.test.js
test/global-engine-settings-ui.test.js
test/global-rules-boundary.test.js
test/global-rules-reset-button.test.js
test/governance-drift-badge.test.js
test/governance-methodology-capability.test.js
test/governed-wrap-ui.test.js
test/governed-wrap.test.js
test/handoff-epoch.test.js
test/handoff-lockfile.test.js
test/handoff-orchestration.test.js
test/handoff-publication.test.js
test/handoff-publish.test.js
test/handoff-repairs.test.js
test/handoff-store.test.js
test/head-requests.test.js
test/health-panel.test.js
test/helpers/lift.js
test/helpers/medusa-proof-phase.js
test/helpers/medusa-wake-matrix.js
test/helpers/preload-medium.js
test/helpers/preload-small.js
test/helpers/ws-test-server.js
test/history-drawer-transcript-badge.test.js
test/history-drawer-uploads.test.js
test/host-check.test.js
test/host-gui-dialog-probe.test.js
test/host-inventory.test.js
test/host-migration-guide.test.js
test/http-settle.test.js
test/https-setup.test.js
test/identity-diagnostics.test.js
test/identity-matrix.test.js
test/import-banner-buttons.test.js
test/ingress-cutover-dry-run.test.js
test/ingress-cutover.test.js
test/ingress-door.test.js
test/ingress-provision.test.js
test/inline-handler-args.test.js
test/install-sh.test.js
test/interactive-decision-routing.test.js
test/isolation-override-banner.test.js
test/landing-projects-load.test.js
test/landing-unreachable-state.test.js
test/landing-wake-hold-badge.test.js
test/landing-wrap-single-flight.test.js
test/launch-baseline.test.js
test/launch-binding-dispatch.test.js
test/launch-binding-guard.test.js
test/launch-binding-routes.test.js
test/launch-bootstrap-wiring.test.js
test/launch-bootstrap.test.js
test/launch-dialog-guard.test.js
test/launch-identity-banner.test.js
test/launch-identity.test.js
test/launch-isolation-inventory.test.js
test/launch-isolation-override-launch.test.js
test/launch-isolation-override-ui.test.js
test/launch-isolation-override.test.js
test/launch-isolation-route.test.js
test/launch-kickoff.test.js
test/launch-mode-picker.test.js
test/launch-mode-settings.test.js
test/launch-panel-fire.test.js
test/launch-preflight-context.test.js
test/launch-preflight-evaluation-failure.test.js
test/launch-preflight.test.js
test/launch-readiness-panel.test.js
test/launch-ready.test.js
test/launch-reconciliation-readback.test.js
test/launch-recovery-batch-read.test.js
test/launch-recovery-clear-batch.test.js
test/launch-recovery-clear-ended.test.js
test/launch-recovery-clear-one.test.js
test/launch-recovery-clear.test.js
test/launch-recovery-clearance-audit.test.js
test/launch-recovery-gate.test.js
test/launch-recovery-held.test.js
test/launch-recovery-migration.test.js
test/launch-recovery-peer-clear.test.js
test/launch-recovery-wake.test.js
test/launch-refusal-toast.test.js
test/launch-review.test.js
test/launch-rule-drift-wiring.test.js
test/launch-rule-drift.test.js
test/launch-sequence-settings.test.js
test/launch-sequence.test.js
test/launch-steps.test.js
test/launch-time-only-warning.test.js
test/launch-unready.test.js
test/launchagent-scan.test.js
test/legacy-claude-md.test.js
test/lifecycle-handoff-daemon.test.js
test/lifecycle-handoff.test.js
test/lift-helper.test.js
test/linux-systemd.test.js
test/live-checkout-banner.test.js
test/logger.test.js
test/login-page.test.js
test/macho-closure.test.js
test/managed-block.test.js
test/master-authority-toggle.test.js
test/master-authority.test.js
test/master-boot-order.test.js
test/master-continuity.test.js
test/master-control-bar.test.js
test/master-drawer-frontend.test.js
test/master-fleet-map.test.js
test/master-guard-source.test.js
test/master-launch-mode-ui.test.js
test/master-launch-mode.test.js
test/master-medusa.test.js
test/master-pane-frontend.test.js
test/master-settings-css-parity.test.js
test/master-settings-frontend.test.js
test/master-settings-mount.test.js
test/master-unknown-frontend.test.js
test/master.test.js
test/medusa-aged-notice.test.js
test/medusa-control-component.test.js
test/medusa-control.test.js
test/medusa-delivery-disposition.test.js
test/medusa-escalation-watchdog.test.js
test/medusa-escalation.test.js
test/medusa-exchanges.test.js
test/medusa-listener.test.js
test/medusa-send-pruned-exchange.test.js
test/medusa-startup-announce.test.js
test/medusa-wake-async.test.js
test/medusa-wake-backoff.test.js
test/medusa-wake-codex-fixtures.test.js
test/medusa-wake-engine-activity.test.js
test/medusa-wake-exchange-proof.test.js
test/medusa-wake-matrix.test.js
test/medusa-wake-stranded-nudge.test.js
test/medusa-wake-triggers.test.js
test/medusa-wake.test.js
test/medusa-watchdog-e2e.test.js
test/medusa-watchdog-meter.test.js
test/medusa-watchdog.test.js
test/merge-authority.test.js
test/merge-inbox-gate.test.js
test/merge-queue-advance.test.js
test/merge-queue-api.test.js
test/merge-queue-github.test.js
test/merge-queue-release-guard.test.js
test/merge-queue-slot.test.js
test/merge-queue-state.test.js
test/merge-queue-store.test.js
test/merge-role-guard.test.js
test/message-size-ui.test.js
test/migration.test.js
test/mobile-toolbar-layout.test.js
test/model-status.test.js
test/model-switch.test.js
test/native-sprint-tracker.test.js
test/next-markdown.test.js
test/offbox-guard.test.js
test/open-install-token.test.js
test/openclaw-approve.test.js
test/openclaw-bridge-port-row.test.js
test/openclaw-cache.test.js
test/openclaw-detect.test.js
test/openclaw-direct-trailing-slash.test.js
test/openclaw-engine.test.js
test/openclaw-html.test.js
test/openclaw-remote.test.js
test/openclaw-routes.test.js
test/openclaw-setup-readme.test.js
test/openclaw-ssh-routes-nonblocking.test.js
test/openclaw-tunnel-state.test.js
test/openclaw-version-route.test.js
test/openclaw-version-row.test.js
test/openclaw-version.test.js
test/operator-bridge-panel.test.js
test/operator-bridge-runbooks.test.js
test/operator-host-topology.test.js
test/operator-proof.test.js
test/orchestration-profiles-store.test.js
test/orchestration.test.js
test/orphan-hooks-banner.test.js
test/orphan-hooks.test.js
test/pane-witness.test.js
test/panel-toggle-rows.test.js
test/password.test.js
test/paste-affordance.test.js
test/paste-rejection-observation.test.js
test/path-tokens.test.js
test/permission-requests.test.js
test/pidfile.test.js
test/pill-ux-contract.test.js
test/pin-https-listener.test.js
test/plan-docs.test.js
test/plan-progress-card.test.js
test/plan-train-card.test.js
test/port-owner-kind-panel.test.js
test/port-scanner.test.js
test/porthub.test.js
test/primary-checkout-guard.test.js
test/prime-golden.test.js
test/prime-hook-output.test.js
test/prime-readiness-gate.test.js
test/prime-rule-count-after-clear.test.js
test/primer-launch-wiring.test.js
test/private-temp-root.test.js
test/product-manual-router.test.js
test/profile-reconciler.test.js
test/project-authority-settings.test.js
test/project-config.test.js
test/project-deliveries-ledger.test.js
test/project-heal.test.js
test/project-import-rules.test.js
test/project-map.test.js
test/project-paths.test.js
test/project-recovery-mode-control.test.js
test/project-recovery-state.test.js
test/project-rules-modal.test.js
test/project-rules-unknown.test.js
test/project-tags-shape.test.js
test/project-update-two-phase.test.js
test/project-version-deps.test.js
test/project-version-files.test.js
test/project-version-require-cycle.test.js
test/project-version.test.js
test/project-view.test.js
test/projects-async-io.test.js
test/projects-dir-upgrade.test.js
test/projects-store-shared-wiring.test.js
test/projects.test.js
test/provenance-engine-carriers.test.js
test/provenance-reporting.test.js
test/provenance-settings.test.js
test/provenance.test.js
test/pty-activity.test.js
test/readme-version-pins.test.js
test/reconnect-policy.test.js
test/record-test-run.test.js
test/recorded-launch-command.test.js
test/recovery-codes-ui.test.js
test/recovery-codes.test.js
test/recovery-default-launch.test.js
test/recovery-default.test.js
test/release-certification-host-checks.test.js
test/release-certification-host-publish.test.js
test/release-certification-isolation.test.js
test/release-certification-publish.test.js
test/release-certification-runner.test.js
test/release-certification-scorecard.test.js
test/release-certification-state-machine.test.js
test/release-certification-store.test.js
test/release-certification-verify.test.js
test/release-mode.test.js
test/release-notes-gate.test.js
test/release-prepare-script.test.js
test/release-readiness.test.js
test/release-recommendation.test.js
test/release-tag-gate.test.js
test/release-workflow.test.js
test/remote-ca-installer.test.js
test/remote-ca-plan-route.test.js
test/remote-output-callers.test.js
test/remote-output.test.js
test/repo-governance-reference.test.js
test/require-graph.test.js
test/reset-admin-store.test.js
test/reset-admin.test.js
test/resource-leases.test.js
test/resume-publication-binding.test.js
test/roadmap-admission.test.js
test/roadmap-sync.test.js
test/roadmap-vercel-guard.test.js
test/role-instruction-guard.test.js
test/rotation-live-check.test.js
test/route-reachability.test.js
test/rule-delivery-receipt.test.js
test/rule-id-display.test.js
test/rule-label-drift.test.js
test/rule-label.test.js
test/rule-receipt-redelivery.test.js
test/scorecard-cache.test.js
test/scorecard-producer.test.js
test/scorecard-publish-metrics.test.js
test/scratch-home-defaults.test.js
test/secret-scan.test.js
test/select-mode-mouse.test.js
test/self-improvement-loop.test.js
test/server-info.test.js
test/server-listen-protocol.test.js
test/server.test.js
test/service-token.test.js
test/session-banner-phone-actions.test.js
test/session-checkout-chip.test.js
test/session-clipboard.test.js
test/session-header-cleanup.test.js
test/session-history-backfill-ui.test.js
test/session-leftovers.test.js
test/session-lifecycle-authz.test.js
test/session-ownership.test.js
test/session-permission-requests-ui.test.js
test/session-reentry.test.js
test/session-relaunch-page.test.js
test/session-relaunch.test.js
test/session-rule-delivery.test.js
test/session-rule-lifecycle.test.js
test/session-rules-bulk.test.js
test/session-rules-channel.test.js
test/session-rules-panel.test.js
test/session-rules-refresh.test.js
test/session-rules-selfimprove.test.js
test/session-rules.test.js
test/session-status-unknown.test.js
test/session-ui.test.js
test/session-unreachable-state.test.js
test/session-update-poll.test.js
test/session-wrap-finalize.test.js
test/session-wrapper.test.js
test/sessions-webui.test.js
test/sessions.test.js
test/sessionstart-prime-claude-hook.test.js
test/setting-disposition.test.js
test/settings-credential.test.js
test/settings-hint-wrapping.test.js
test/settings-index-toggle-render.test.js
test/settings-launch-mode-render.test.js
test/settings-modal-silentprime.test.js
test/settings-narrow-overflow.test.js
test/settings-silent-prime-render.test.js
test/settings-tailscale-mode-render.test.js
test/setup-credential-uniformity.test.js
test/setup-credential.test.js
test/setup-engine-picker.test.js
test/setup-ingress-state-endpoint.test.js
test/setup-provisioning.test.js
test/setup-scan-own-install.test.js
test/setup-wizard-dir-error.test.js
test/setup-wizard-engines.test.js
test/setup-wizard-git-unknown.test.js
test/setup-wizard-https.test.js
test/setup-wizard-login-gate.test.js
test/setup-wizard-notice-wording.test.js
test/setup-wizard.test.js
test/shared-doc-broadcast-coalesce.test.js
test/shared-doc-broadcast-scope.test.js
test/shared-docs-access.test.js
test/shared-docs-attest-route.test.js
test/shared-docs-events.test.js
test/shared-docs-notice-routing.test.js
test/shared-docs-upkeep.test.js
test/sidecar-openclaw.test.js
test/sidecar-panel.test.js
test/sidecar-pills.test.js
test/sidecar.test.js
test/signed-in-operator-routes.test.js
test/silent-prime-drop-warning.test.js
test/skip-ledger.json
test/soak-attest-bridge.test.js
test/soak-browser.test.js
test/soak-bundle.test.js
test/soak-cli.test.js
test/soak-driver.test.js
test/soak-executors.test.js
test/soak-faults.test.js
test/soak-guest.test.js
test/soak-integrity.test.js
test/soak-judge.test.js
test/soak-local.test.js
test/soak-medusa-stub.test.js
test/soak-repos-attach.test.js
test/soak-repos.test.js
test/soak-schedule.test.js
test/soak-stub-engine.test.js
test/ssh-target-safety.test.js
test/startup-control-codex.test.js
test/startup-control-launch.test.js
test/startup-control.test.js
test/startup-dialog-mid-session.test.js
test/startup-dialog.test.js
test/startup-prompt-api.test.js
test/startup-prompt-editor.test.js
test/startup-prompt-service.test.js
test/startup-prompt-store.test.js
test/store-activity.test.js
test/store-auth-sessions.test.js
test/store-bridge-migration.test.js
test/store-check-watch-migration.test.js
test/store-control-migration.test.js
test/store-doclocks.test.js
test/store-eval-audit.test.js
test/store-globalrules.test.js
test/store-groups.test.js
test/store-isolation-override-migration.test.js
test/store-learnings.test.js
test/store-medusa-exchange-migration.test.js
test/store-merge-queue-migration.test.js
test/store-openclaw.test.js
test/store-portleases.test.js
test/store-projects.test.js
test/store-recovery-codes.test.js
test/store-session-mode.test.js
test/store-session-stop-provenance-migration.test.js
test/store-sessions.test.js
test/store-shared-doc-events-migration.test.js
test/store-shared-doc-prefs-migration.test.js
test/store-shareddocs.test.js
test/store-users.test.js
test/store.test.js
test/stranded-check.test.js
test/stranded-configs.test.js
test/stranded-open-pr.test.js
test/stranded-wraps-gate.test.js
test/stranded-wraps-ui.test.js
test/stranded-wraps.test.js
test/strip-ai-coauthors-script.test.js
test/suite-evidence.test.js
test/sw-register.test.js
test/switchboard-swarm-stats.test.js
test/system-health.test.js
test/system.test.js
test/tailnet-cutover.test.js
test/tailscale-ingress.test.js
test/tangleclaw-home.test.js
test/tc-branch-check.test.js
test/tc-check-verb.test.js
test/tc-cli-reference.test.js
test/tc-cli.test.js
test/tc-control.test.js
test/tc-docs-scorecard-merge-wiring.test.js
test/tc-finalize-verb.test.js
test/tc-pane-identity.test.js
test/tc-primer.test.js
test/tc-queue-verb.test.js
test/tc-require-graph.test.js
test/tc-rotation-verb.test.js
test/tc-start-cli.test.js
test/tc-verbs.test.js
test/tc-workload-verb.test.js
test/tcc-folders.test.js
test/team-templates.test.js
test/temp-repo.test.js
test/terminal-chrome-slice2.test.js
test/terminal-drag-copy.test.js
test/terminal-frame-wiring.test.js
test/terminal-identity.test.js
test/terminal-math.test.js
test/terminal-selection-fix.test.js
test/terminal-tap-link.test.js
test/terminal-touch-scroll.test.js
test/test-results.test.js
test/test-skip-audit.test.js
test/test-stage.test.js
test/test-tiers.test.js
test/test-touched.test.js
test/theme-contrast.test.js
test/tick-meter.test.js
test/tiers.json
test/tmux-draft-capture.test.js
test/tmux-guard.test.js
test/tmux-named-paste-buffer.test.js
test/tmux-pane-start-commands.test.js
test/tmux-read-pane-async.test.js
test/tmux-session-names.test.js
test/tmux-tcc.test.js
test/tmux.test.js
test/tracked-carrier-identity.test.js
test/transcript.test.js
test/ttyd-attach-sync.test.js
test/ttyd-attach.test.js
test/ttyd-bind.test.js
test/ttyd-churn.test.js
test/ttyd-plist.test.js
test/ttyd-runtime.test.js
test/ttyd-watcher.test.js
test/tunnel-monitor.test.js
test/tunnel.test.js
test/ub-self-update-action.test.js
test/ui-openclaw.test.js
test/untrusted-pr-review.test.js
test/update-applier-authored-content.test.js
test/update-applier-global-rules-carry.test.js
test/update-applier-managed-carrier.test.js
test/update-applier.test.js
test/update-beacon-pages.test.js
test/update-beacon.test.js
test/update-checker.test.js
test/update-prompt-guards.test.js
test/update-prompt-path.test.js
test/update-release-link.test.js
test/upload-modal-frontend.test.js
test/upload-pills.test.js
test/uploads-fs.test.js
test/uploads.test.js
test/upstream-drift-workflow.test.js
test/upstream-observer.test.js
test/usage-windows.test.js
test/v5670-car67a-control-roadmap-merge-gate.test.js
test/version-bump-fail-closed.test.js
test/version-bump-fragments.test.js
test/version-bump-package-json.test.js
test/version-bump-pyproject.test.js
test/version-bump-release-gate.test.js
test/version-visibility.test.js
test/visual-viewport-layout.test.js
test/with-scratch-server.test.js
test/workflow-action-pins.test.js
test/workflow-queue-trigger.test.js
test/workload-compose.test.js
test/workload-fleet.test.js
test/workload-guidance.test.js
test/workload-nudge-facts.test.js
test/workload-nudge-settings.test.js
test/workload-nudge.test.js
test/workload-receipts.test.js
test/workspace-rename-check.test.js
test/wrap-bump-level-askmode.test.js
test/wrap-changelog-coverage.test.js
test/wrap-changelog-transaction.test.js
test/wrap-confirm-calls-defined.test.js
test/wrap-consecutive-step-delivery.test.js
test/wrap-continuity-session-provenance.test.js
test/wrap-default-pipeline.test.js
test/wrap-delivery-receipt.test.js
test/wrap-drawer-docs-parity.test.js
test/wrap-drawer-select-a11y.test.js
test/wrap-drawer-triggers.test.js
test/wrap-drawer.test.js
test/wrap-ends-session.test.js
test/wrap-engine-agnostic.test.js
test/wrap-file-ownership.test.js
test/wrap-file-safety.test.js
test/wrap-git-range.test.js
test/wrap-handback.test.js
test/wrap-indicator-card.test.js
test/wrap-intent-cancel.test.js
test/wrap-learnings-coverage.test.js
test/wrap-methodology-authority.test.js
test/wrap-needs-operator-live.test.js
test/wrap-pipeline-lane.test.js
test/wrap-pipeline-prompts.test.js
test/wrap-pipeline.test.js
test/wrap-plan-mode-precheck.test.js
test/wrap-popover-markup.test.js
test/wrap-popover-view.test.js
test/wrap-pr-status.test.js
test/wrap-release-decision.test.js
test/wrap-release-prepare.test.js
test/wrap-release-recommendation-step.test.js
test/wrap-rename-pathspec.test.js
test/wrap-rule-proposal-widget.test.js
test/wrap-run-controller.test.js
test/wrap-run-reattach.test.js
test/wrap-run-registry.test.js
test/wrap-run-session-wiring.test.js
test/wrap-secret-check.test.js
test/wrap-session-scope.test.js
test/wrap-skip-continue-label.test.js
test/wrap-state.test.js
test/wrap-step-ai-content.test.js
test/wrap-step-commit-autopr.test.js
test/wrap-step-continuity-write.test.js
test/wrap-step-exec-timeout.test.js
test/wrap-step-features-toc.test.js
test/wrap-step-git-range-killed.test.js
test/wrap-step-index-describe.test.js
test/wrap-step-killed-vs-failed.test.js
test/wrap-step-learnings-db-write.test.js
test/wrap-step-overrides.test.js
test/wrap-step-pr-merge.test.js
test/wrap-step-preflight.test.js
test/wrap-step-priming-roll.test.js
test/wrap-step-project-map.test.js
test/wrap-step-source-paths.test.js
test/wrap-step-status-vocabulary.test.js
test/wrap-steps-date.test.js
test/wrap-stream-client.test.js
test/wrap-stream-event-vocabulary.test.js
test/wrap-stream-sw-cache.test.js
test/wrap-tc-owned-paths.test.js
test/wrap-untrack-offer-ui.test.js
test/wrap-upstream-provenance.test.js
test/ws-unix-client.test.js
test/z-index-scale.test.js
```

</details>

### docs/ (115 tracked paths)

<details>
<summary>Tracked inventory</summary>

```text
docs/adr/0001-symmetric-capability-gates.md
docs/adr/0002-wrap-pipeline-contract.md
docs/adr/0003-ingress-model.md
docs/adr/0004-auth-2-basic-auth-gate.md
docs/adr/0005-service-tokens.md
docs/adr/0006-key-ref-hygiene.md
docs/adr/0007-project-map-freshness.md
docs/adr/0008-project-master-session-model.md
docs/adr/0009-secure-by-default.md
docs/adr/0010-one-update-mechanism.md
docs/adr/0011-prawduct-boundary.md
docs/adr/0012-enforcement-adds-no-install-step.md
docs/adr/0013-settings-take-effect-or-say-why-not.md
docs/adr/0014-dual-key-review-for-untrusted-prs.md
docs/adr/0015-tangleclaw-owns-authentication.md
docs/adr/0016-tier-1-auth-build-decisions.md
docs/adr/0017-phased-launch.md
docs/adr/0018-owned-ttyd-runtime.md
docs/adr/0019-generated-file-provenance.md
docs/adr/0020-session-workload-receipts.md
docs/adr/0021-public-release-certification-scorecard.md
docs/adr/0023-master-mediated-operator-bridge.md
docs/adr/0024-roadmap-release-grouping.md
docs/adr/0025-codex-cold-start-wake-and-pane-refusal.md
docs/ai-doc-manifest.json
docs/areas.json
docs/auth-status-surfacing.md
docs/bridge-authority-tiers.md
docs/caddy-drift-check.md
docs/changelog-fragments.md
docs/check-watch.md
docs/configuration-reference.md
docs/control-state.md
docs/coordinator-rotation.md
docs/dependency-bump-audit.md
docs/discord-operator-notifications.md
docs/engine-guide.md
docs/eval-audit-mode.md
docs/external-code-security.md
docs/fleet-workload.md
docs/host-migration.md
docs/lane-ci-evidence.md
docs/macos-tcc-automations.md
docs/manual-manifest.json
docs/medusa-delivery.md
docs/medusa-wake-measurements.md
docs/merge-queue.md
docs/openclaw-setup.md
docs/operator-bridge-helper.md
docs/operator-bridge.md
docs/prawduct-feedback.md
docs/primary-checkout-guard.md
docs/recovery.md
docs/release-process.md
docs/releases/index.md
docs/releases/v5.30-status.md
docs/releases/v5.31-status.md
docs/releases/v5.32-status.md
docs/releases/v5.33-status.md
docs/releases/v5.33.1-status.md
docs/releases/v5.34-status.md
docs/releases/v5.35-status.md
docs/releases/v5.36-status.md
docs/releases/v5.38-status.md
docs/releases/v5.39-status.md
docs/releases/v5.40-status.md
docs/releases/v5.41-status.md
docs/releases/v5.42-status.md
docs/releases/v5.43-status.md
docs/releases/v5.44-status.md
docs/releases/v5.45-status.md
docs/releases/v5.46-status.md
docs/releases/v5.46.1-status.md
docs/releases/v5.47-status.md
docs/releases/v5.47.1-status.md
docs/releases/v5.48-status.md
docs/releases/v5.48.1-status.md
docs/releases/v5.49.0-status.md
docs/releases/v5.50.0-status.md
docs/releases/v5.51.0-status.md
docs/releases/v5.52.0-status.md
docs/releases/v5.52.0.md
docs/releases/v5.53.0-status.md
docs/releases/v5.53.0.md
docs/releases/v5.53.1-status.md
docs/releases/v5.53.1.md
docs/releases/v5.54.0-status.md
docs/releases/v5.54.0.md
docs/releases/v5.55.0-status.md
docs/releases/v5.55.0.md
docs/releases/v5.56.0-status.md
docs/releases/v5.56.0.md
docs/releases/v5.65.0.md
docs/releases/v5.66.0.md
docs/releases/v5.67.0.md
docs/releases/v5.68.0.md
docs/releases/v5.69.0.md
docs/releases/v5.70.0.md
docs/releases/v5.71.0.md
docs/runbooks/activate-the-operator-bridge.md
docs/runbooks/dashboard-blank.md
docs/runbooks/put-back-the-build-before-the-operator-bridge.md
docs/runbooks/release-certification.md
docs/runbooks/roll-back-the-operator-bridge.md
docs/runbooks/roll-back-the-owned-ttyd.md
docs/runbooks/roll-out-the-owned-ttyd.md
docs/runbooks/soak-install-the-candidate.md
docs/runbooks/soak-run-sample-and-bundle.md
docs/runbooks/stand-up-a-new-agent-fleet.md
docs/session-finalize.md
docs/session-rules-self-improvement.md
docs/setup-guide.md
docs/tc-cli-reference.md
docs/test-pyramid.md
docs/user-guide.md
```

</details>

### Selected boundary modules (53 tracked paths)

<details>
<summary>Tracked inventory</summary>

```text
lib/admin-credential.js
lib/auth-gate.js
lib/auth-identity.js
lib/auth-session.js
lib/behind-origin.js
lib/bridge-helper/config.js
lib/caddy-drift.js
lib/caddy.js
lib/checkout-fingerprint.js
lib/checkout-fleet.js
lib/checkout-freshness.js
lib/checkout-layout.js
lib/checkout-ownership.js
lib/checkout-state.js
lib/checkout-summary.js
lib/control-auth.js
lib/doc-router.js
lib/git-probe.js
lib/github-checks.js
lib/github-facts.js
lib/host-check.js
lib/host-gui-dialog-probe.js
lib/host-inventory.js
lib/http-settle.js
lib/https-setup.js
lib/ingress-door.js
lib/ingress-provision.js
lib/launch-binding-guard.js
lib/linux-systemd.js
lib/open-install-token.js
lib/openclaw-approve.js
lib/openclaw-detect.js
lib/openclaw-html.js
lib/openclaw-remote.js
lib/openclaw-version.js
lib/password.js
lib/port-scanner.js
lib/porthub.js
lib/release-certification/host-checks.js
lib/release-certification/host-publish.js
lib/remote-ca-installer.js
lib/server-info.js
lib/service-token.js
lib/setup-credential.js
lib/shared-docs-access.js
lib/shared-docs-http.js
lib/shared-docs.js
lib/tailscale-ingress.js
lib/update-applier.js
lib/update-checker.js
lib/upload-pills.js
lib/uploads-fs.js
lib/uploads.js
```

</details>

### Root contracts (16 tracked paths)

<details>
<summary>Tracked inventory</summary>

```text
.gitattributes
.gitignore
.node-version
.testament.toml
CHANGELOG.md
CLAUDE.md
CODE_OF_CONDUCT.md
CONTRIBUTING.md
FEATURES.md
LICENSE
PROJECT-MAP.md
README.md
ROADMAP.md
SECURITY.md
server.js
version.json
```

</details>
