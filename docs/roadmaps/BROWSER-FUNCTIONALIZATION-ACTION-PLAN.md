# Browser Functionalization Action Plan

**Status:** active program  
**Coordination:** GitHub issue #50  
**Evidence basis:** repository audit verified against the active tree on
2026-07-23

## Objective

Produce three independently testable deliverables:

1. **SilkSurf Browser** -- a Rust-native shell with tabs, profiles, permissions,
   downloads, session state, and supervised page-engine processes.
2. **SilkSurf Chat** -- a native API-backed AI-chat client whose mounted render
   state is bounded by the viewport rather than total conversation length.
3. **SilkSurf Native Engine** -- the existing Rust engine, preserved as a
   trusted/local/research backend and matured independently toward public-web
   compatibility and isolation.

The project must not make a functional browser contingent on the native engine
first reaching Chrome/WebKit parity. Conversely, compatibility-engine work must
not displace the native engine's strongest retained-rendering and cleanroom
research assets.

## Verified premises

The diagnostic premises are established:

- the integrated application path is real,
- React-class mount/event/commit/repaint works for retained fixtures,
- the current shell and page runtime are single-process and single-view,
- the WPT-labelled score is synthetic rather than upstream interoperability,
- the test262 full baseline is recorded but not currently reproducible from a
  clean checkout because the corpus is not vendored,
- h2spec remains a scaffold,
- loader caps and missing lifecycle/API surfaces block broad production SPAs,
- no renderer/site-isolation boundary, CORS, CSP, or SRI enforcement exists,
- event-driven GUI waking is already implemented,
- 10 us is a bounded CPU microbudget, not an end-to-end input-to-photon claim.

## Architecture principle

The shell/engine interface is view-oriented and process-neutral. It does not
expose engine-specific DOM, JavaScript, CSS, or layout types.

### Commands

- create and close a view,
- navigate, reload, stop, back, and forward,
- resize, scale, visibility, and focus,
- pointer, keyboard, text, IME, and composition input,
- permission and file-chooser decisions,
- graceful shutdown and forced termination.

### Events

- title, URL, favicon, history, and load-state changes,
- frame handle, damage regions, generation, and release state,
- permission, download, file chooser, popup, and new-view requests,
- console/diagnostic output,
- resource and performance metrics,
- crash, hang, and protocol error.

Frame transport prefers platform handles/DMA-BUF where practical and sealed
shared memory otherwise. Every buffer has explicit generation, ownership, and
release semantics.

## Decision gates

The compatibility-backend recommendation is not frozen until comparable spikes
run against one harness.

### DG-1 -- native engine behind IPC

Move the existing page runtime into a supervised child process without changing
its semantics. Prove navigation, input, frame transfer, clean shutdown, crash
recovery, and tab reload. Measure startup, RSS, copies, frame submission, and
latency distributions.

**Output:** engine protocol v1 and minimum shell-owned state.

### DG-2 -- WPE WebKit Linux/Wayland spike

Embed one WPE view in the SilkSurf shell. Exercise resize, input, navigation,
profile persistence, streaming, secure WebSocket, downloads, file chooser,
accessibility/devtools exposure, and crash handling. Record package footprint,
RSS, startup, copy count, and security-update obligations.

### DG-3 -- Wry, Servo, and CEF comparison

Run the same acceptance and measurement harness. Classify each backend as
primary, fallback, experimental, or rejected based on compatibility, process
model, profile control, frame integration, platform reach, update burden, RSS,
startup, and ChatGPT acceptance results.

No fixed 15-crate decomposition follows from the audit. Crates are introduced
only when the spikes establish stable ownership and dependency boundaries.

## Phase 0 -- authoritative status

- reconcile root README, architecture, JavaScript guide, app README, scorecards,
  and manifests,
- generate or mechanically check current status,
- reject stale fixture counts, toolchain pins, integration plans, and retired VM
  claims in the fast gate,
- state corpus availability and denominators for every conformance percentage,
- keep event-driven waking marked complete.

**Exit:** current code, manifests, scorecards, and canonical docs agree.

## Phase 1 -- protocol and process isolation

- specify IDs, messages, capabilities, state machines, errors, quotas, and
  buffer ownership,
- add serialization, version-negotiation, malformed-message, and fuzz tests,
- supervise the native engine process,
- detect crash and hang, terminate, and reload without losing the shell,
- define shell ownership of profiles, permissions, downloads, and session state,
- complete DG-1 through DG-3 and record an ADR.

**Exit:** terminating a page engine does not terminate the shell or unrelated
views.

## Phase 2 -- functional browser shell

Replace the single `BrowserState` model with multi-view shell state and add:

- tabs, windows, popup policy, and view lifecycle,
- persistent and private profiles,
- session restoration, history, and bookmarks,
- downloads and file chooser,
- permissions and site/certificate information,
- zoom, scale, context menus, clipboard/selection routing, and accessibility,
- engine-specific developer tools,
- per-tab CPU/RSS/frame metrics and renderer-crash UI,
- scripted end-to-end browser-operation tests.

**Exit:** multiple views navigate independently, profile data remains isolated,
and one engine failure is recoverable.

## Phase 3 -- ChatGPT website acceptance ladder

Against the selected compatibility backend and a persistent test profile:

1. authenticate,
2. preserve session across reload and restart,
3. open/create a conversation,
4. send text and receive a streamed response,
5. cancel and retry,
6. select/copy/edit with IME and composition,
7. upload and download a file,
8. exercise secure WebSocket paths,
9. recover after an engine crash,
10. record memory and frame behavior for large conversations.

Website mode cannot guarantee constant complexity for application-owned DOM,
framework state, or reconciliation. Claims are limited to measured behavior.

## Phase 4 -- native AI-chat mode

### Data model

Persist immutable conversation/message/block records in SQLite WAL. Separate raw
source, parsed block metadata, completion state, attachments, and transient
render state.

### Virtualization

- retain only visible blocks and bounded overscan,
- index measured block heights with a Fenwick or segment tree,
- resolve scroll offset to block in logarithmic time,
- apply height corrections without linear transcript walks,
- keep composer layout and paint separate from transcript layout and paint.

### Streaming

- append service deltas to one active message buffer,
- coalesce UI commits to at most one per display frame,
- reparse only the incomplete trailing Markdown block,
- update one height entry and one visible damage region,
- never reflow completed offscreen messages.

### Deferred work

Code highlighting, image decode, tables, and other heavy blocks execute only near
the viewport and are cached by content hash.

### Acceptance datasets

Use deterministic 100-, 1,000-, and 10,000-turn fixtures with prose, code,
tables, links, images, and tool outputs.

**Exit:** mounted objects are bounded by viewport/overscan, memory slope is
measured, and typing/frame latency does not materially degrade as total turns
grow.

## Phase 5 -- native engine graduation

### Interoperability

- implement an upstream WPT product adapter,
- emit standard testharness/reftest/crash/timeout results,
- keep synthetic fixtures as regression tests rather than compatibility metrics,
- make test262 corpus acquisition/pinning reproducible,
- replace the h2spec stub with a reproducible target.

### Loader and event loop

- parser-blocking, async, defer, and module ordering,
- document lifecycle and browser task sources,
- import maps, dynamic import, `import.meta`, and top-level await,
- streaming response bodies and cancellation,
- normal-path HTTP/2, workers, and service workers.

### Security and reliability

- renderer sandbox and origin broker,
- same-origin enforcement, CORS, CSP, SRI, mixed content, and referrer policy,
- CPU, memory, response, script, DOM, selector, layout, image, and raster quotas,
- hang termination and recovery.

### Web API and presentation

- Selection/Range, contenteditable, `beforeinput`, clipboard, IME/composition,
- WebCrypto `subtle`, IndexedDB, structured clone, Cache Storage,
- scrolling/overflow/positioning/stacking/transforms/containment/animation,
- international text, accessibility-tree synchronization, and required media
  surfaces.

**Exit:** compatibility is stated by upstream focus area, corpus, denominator,
real-site ladder, and security boundary, never by an unqualified
"standards-compliant" label.

## Performance contract

Record independent p50/p95/p99/max distributions for:

- browser-chrome bounded CPU work,
- input dispatch,
- JavaScript or native-model commit,
- layout,
- raster,
- frame submission,
- compositor-visible presentation,
- network/service time-to-first-token.

Initial validation targets:

- chrome bounded CPU: p50 <= 10 us, p99 <= 50 us,
- native-chat input to model commit: p50 < 1 ms, p95 < 2 ms,
- text-only commit to submit: p95 < 0.5 ms,
- complex visible-region update to submit: p95 < 2 ms,
- no normal interaction task above 50 ms,
- transcript mounted state independent of total turn count.

These are hypotheses to test and revise, not pre-certified guarantees.

## Sequencing rules

- macro traces and failed acceptance rungs choose optimization work,
- every behavior claim retains a reproducer,
- every percentage names date, corpus, scope, and denominator,
- security boundaries precede arbitrary-public-web claims,
- compatibility-engine updates are continuing security maintenance,
- the local full gate remains authoritative until an intentionally equivalent
  hosted/self-hosted gate is adopted.

## Immediate child issues

After the Phase 0 reconciliation PR:

1. engine protocol v1 specification,
2. native runtime process-extraction spike,
3. WPE embedding spike,
4. comparable compatibility-backend scorecard,
5. upstream WPT product-adapter skeleton,
6. native-chat data model and 10,000-turn virtualization benchmark.
