# SPA Capability Roadmap

**Date**: 2026-07-12
**Scope**: the engineering path from a static-page pipeline to a browser
where large client-rendered applications function.
**Evidence base**: a live-tree capability audit (file:line citations
below reflect the 2026-07-12 tree; symbol names are the durable
anchors) plus a falsification pass over the prior roadmaps
(docs/roadmaps/DEBT-RECONCILIATION-ROADMAP.md;
docs/archive/roadmaps/SNAZZY-WAFFLE-COMPLETION.md).

## Acceptance frame: the site ladder

Each rung is a reproducible acceptance target; a rung is claimed only
with retained evidence (scripted load + observed behavior), never by
inspection of the code.

1. **static-document rung** -- example.com-class pages render. HOLDS
   today (headless smoke exits 0; wpt fixtures 63/63).
2. **enhanced-static rung** -- Wikipedia-class pages: complex CSS,
   progressive-enhancement scripts that query and mutate the DOM.
   Requires W2 (selectors, innerHTML reparse).
3. **local-spa rung** -- a self-hosted chat-clone SPA (React/Next.js
   class): hydration, delegated event listeners, client routing,
   streamed responses. Requires W1 + W2 + W3 + W4. This rung is the
   honest proxy for chatgpt.com with no bot-wall confound.
4. **live-spa rung** -- github.com-class production SPA with login.
   Additionally requires the deferred auth substrate (WebCrypto,
   persistent storage hardening).
5. **chatgpt-com rung** -- additionally requires surviving CDN bot
   checks (TLS fingerprint, challenge JS) and sustained boa throughput
   on multi-megabyte bundles. Gated by the two named spikes below.

## Verified capability baseline (what already works)

The buildout stands on mechanisms that exist and are tested in-tree;
re-verifying these is not part of the plan:

- Host scheduler for setTimeout/setInterval/requestAnimationFrame with
  deadline arithmetic (silksurf-js/src/boa_backend/mod.rs,
  HostScheduler); boa microtask drain after eval and every host tick.
- GUI pumping: winit wake deadlines drive run_host_callbacks with a
  per-tick budget (crates/silksurf-app/src/main.rs,
  runtime_repaint.rs).
- Incremental repaint: DOM mark_dirty -> take_dirty_nodes -> fused
  incremental style/layout -> damage-rect rasterization
  (crates/silksurf-app/src/runtime_repaint.rs;
  crates/silksurf-engine/src/lib.rs render_document_incremental).
- Live DOM bridge: createElement/appendChild/setAttribute/textContent
  mutate the shared Arc<Mutex<Dom>> and mark dirty
  (silksurf-js/src/boa_backend/dom_bridge.rs).
- CSS engine: full selector parse/match (crates/silksurf-css/src/
  selector.rs, matching.rs), inline style attribute honored by the
  cascade (style.rs apply_inline_style_attribute), custom properties,
  calc(), media query evaluation (media.rs).
- Networking: redirects, gzip/deflate/brotli, partitioned cookie store
  shared with document.cookie, h2 batch client
  (crates/silksurf-net/src/).
- Conformance harnesses: wpt_runner (63 synthetic fixtures,
  crates/silksurf-engine/conformance/wpt/) and test262_boa
  (silksurf-js/src/bin/test262_boa.rs).

## Load-bearing gaps (falsified against the tree, 2026-07-12)

| Gap | Mechanism today | Evidence |
|---|---|---|
| G1 event propagation | dispatch fires target-only; no capture/bubble, no currentTarget/stopPropagation/preventDefault | dom_bridge.rs dispatch_event |
| G2 input-to-JS bridge | GUI clicks/keys handled entirely in Rust; JS listeners never fire from real input | crates/silksurf-app/src/input.rs (no js_ctx references) |
| G3 innerHTML | setter writes TEXT (wired to text_content_set_native); no fragment reparse | dom_bridge.rs inner_html_set |
| G4 element.style / dataset | dead empty JS objects; writes neither style nor invalidate | dom_bridge.rs style/dataset object creation |
| G5 bridge selectors | single #id/.class/tag matcher, not the silksurf-css engine | dom_bridge.rs matches_selector |
| G6 networking dynamics | fetch/XHR synchronous + blocking, promise pre-resolved; ReadableStream stub; WebSocket one-shot roundtrip; no SSE | mod.rs fetch_sync; websocket.rs |
| G7 API surface | no getComputedStyle, matchMedia, history.pushState, queueMicrotask; storage in-memory per-context | grep zero |

## Workstreams

**Status (2026-07-12): W1 through W4 LANDED.** Gate evidence: make full
green (611 workspace tests, 0 failures); wpt scorecard 70/70 (was 63,
adds seven js_* fixtures); headless render of a fixture exercising
delegated click listeners, innerHTML swap, pushState, localStorage, and
style writes exits 0 with "Pipeline complete"; `make gui-probe --probe
smoke` presents frames against the same fixture over Wayland. Scope cuts
made during execution are named in the deferral list below, not silently
dropped.

Ordering: W1a -> W1b -> W2c -> W2a -> W2b -> W3a -> W3b/c/d -> W4.
Every sub-item lands separately behind `make check && make test`; the
wpt scorecard pass count strictly increases where fixtures are added and
never regresses; `make full` before any merge-ready claim.

### W1 dom-event-propagation + native-input-event-bridge

- **event-propagation-dispatcher** (silksurf-js/src/boa_backend/
  event_dispatch.rs, new): capture/target/bubble walk over an ancestor
  path snapshotted from the Dom with the lock released before any
  listener runs. Listener registry stays JS-side (GC-rooted); values
  become {bubble, capture} arrays; addEventListener accepts
  bool-or-{capture, once}. Event object carries type/target/
  currentTarget/eventPhase/bubbles/cancelable/defaultPrevented plus
  payload fields; stopPropagation/stopImmediatePropagation/
  preventDefault are plain natives over own-property flags.
  Per-listener error catch. Gate: ordering, stop-propagation,
  once, listener-exception, and re-entrant dispatch unit tests.
- **synthetic-event-entrypoint**: SilkContext::dispatch_dom_event
  (drains microtasks after) and has_dom_listeners backed by a
  Rust-side listened-types set so listener-free pages pay zero
  synthesis cost. Gate: unit tests + zero-listener fast path test.
- **native-input-event-synthesis** (crates/silksurf-app/src/
  js_events.rs, new; input.rs edits): mousedown/mouseup/click on
  hit-tested nodes, keydown/keyup/input/change on focused editing,
  submit on form submission. JS handlers fire first; preventDefault
  suppresses the existing native action (link follow, form submit,
  text edit); otherwise native behavior is unchanged. Dispatch runs
  outside any Dom lock; repaint rides the dirty-node path. Gate:
  click-preventDefault-blocks-navigation app test; wpt fixtures
  js_event_bubbling.html, js_click_prevent_default.html.

### W2 selector-engine-reuse + innerhtml-fragment-reparse + inline-style-attribute-writeback

- **selector-engine-reuse** (cheapest, first): silksurf-js gains the
  silksurf-css dependency (verified acyclic); querySelector(All)/
  closest/matches parse via parse_selector_list_with_interner and
  match via matches_selector_list; capped per-context selector parse
  cache. Gate: descendant/combinator/attribute selector unit tests;
  fixture js_query_selector_complex.html.
- **innerhtml-fragment-reparse**: parse_fragment_into in
  crates/silksurf-html/src/treesink.rs (html5ever parse_fragment with
  the target's tag as context element, into a scratch Dom via the
  existing SilkDomBuilder); import_subtree in silksurf-dom re-creates
  nodes through existing create/set/append APIs so mark_dirty fires
  for free. innerHTML setter clears children then splices. Gate:
  table-context fragment test; import dirty/generation test; fixture
  js_innerhtml_reparse.html.
- **inline-style-attribute-writeback**: style and dataset become JS
  proxies whose traps read/upsert the element's style attribute (or
  data-* attributes) through set_attribute -- the cascade already
  honors inline style, so invalidation and incremental repaint need
  zero engine changes. setProperty/getPropertyValue/removeProperty/
  cssText exposed. Gate: style-write-reflects-in-attribute test;
  fixture js_style_write_repaint.html.

### W3 host-net-completion-queue

- **net-completion-queue** (silksurf-js/src/boa_backend/net_queue.rs,
  new): worker std::thread runs the blocking BasicClient::fetch;
  completions cross an mpsc channel; promise resolvers are stored
  JS-side (GC-rooted) keyed by request id; run_host_callbacks drains
  completions and resolves via the job queue. In-flight work counts as
  pending host work with a 10 ms poll deadline (deliberate v1; a real
  waker via EventLoopProxy is a named deferral). fetch honors
  method/body/headers from init; XHR fires its readystatechange
  sequence through an EventTarget mixin for non-DOM targets. Teardown:
  SilkContext::cancel_pending_net called from page teardown. Gate:
  promise-pending-until-drain proof; abort test; drive_until_done
  termination test (CLI must not hang on in-flight work).
- **readablestream-chunked-delivery**: reader.read() resolves from a
  per-request chunk queue or parks its resolver; v1 slices the
  completed body (socket-level streaming is a named deferral inside
  BasicClient). Gate: chunk-sequence-then-done test.
- **websocket-persistent-session** (crates/silksurf-net/src/
  websocket_session.rs, new; the one-shot probe stays for its tests):
  background thread owning a current-thread tokio runtime +
  tokio-tungstenite, outbound mpsc selected against inbound frames;
  JS WebSocket gets real readyState and open/message/close/error
  events through the completion queue; Drop shuts the session down.
  Gate: loopback echo open/message/close ordering test.
- **eventsource-sse** (crates/silksurf-net/src/sse.rs, new): SSE field
  parser (data:/event:/id:/retry:, dispatch on blank line) as a pure
  function with table-driven tests; incremental-read GET; JS
  EventSource on the same queue. Gate: two-event stream test.

### W4 dom-api-surface

- **queuemicrotask-nativejob**: global wrapping the callback in a boa
  NativeJob. Gate: microtask-before-setTimeout(0) ordering test.
- **computed-style-provider-callback**: the app installs
  Fn(NodeId, &str) -> Option<String> capturing the Dom and current
  StyleIndex, calling compute_style_for_node_with_index on demand;
  getComputedStyle returns a proxy over the provider. The supported
  property list is scoped explicitly in the doc comment. Gate:
  computed-style-reflects-live-inline-write test; fixture
  js_get_computed_style.html.
- **matchmedia-evaluator-exposure**: silksurf-css media.rs evaluator +
  SilkContext::set_viewport (app calls it on resize). Gate:
  two-viewport test; fixture js_match_media.html.
- **same-document-history-intents**: pushState/replaceState update
  JS-side state and enqueue HistoryIntent; the app drains intents each
  tick into session history; back/forward to a same-document entry
  dispatches popstate (W1 dispatcher) instead of reloading. State via
  serde_json (structured-clone-lite, documented limitation). Gate:
  pushState/popstate roundtrip test.
- **origin-keyed-storage-writeback** (crates/silksurf-app/src/
  profile.rs, new): $XDG_DATA_HOME/silksurf/storage/<origin-hash>.json
  with atomic temp+rename writes and an --ephemeral escape hatch;
  SilkContext preloads the map and exposes take_storage_if_dirty;
  the app flushes debounced on tick and on teardown. No new deps.
  Gate: JSON roundtrip through a temp dir.

## Carried-forward debt (from DEBT-RECONCILIATION-ROADMAP.md, verified open)

These stay sequenced in the debt roadmap; listed here because they
share files with the workstreams above and should land opportunistically:

- treebuilder-document-expect-annotation
  (crates/silksurf-html/src/tree_builder.rs -- same crate as W2a).
- wayland-shm-safe-wrapper (crates/silksurf-gui/src/wayland_shm.rs).
- sendptr-send-sync-soundness-proof (crates/silksurf-render/src/lib.rs).
- deny-policy-hardening (blocked on two informational RUSTSEC ignores).
- msrv-exercise-policy (make msrv target).
- diff-analysis planning-doc re-homing and the other cleanroom
  physical relocations.

## Named deferrals (not in this execution; each needs its own landing)

- **boa-bundle-throughput-spike** -- RUN 2026-07-12; verdict and numbers
  in docs/findings/boa-react-bundle-throughput.md. React 18 mounts and
  commits into the silksurf DOM (after ownerDocument and DOM-interface
  constructor stubs landed); boa evaluates minified framework code at
  roughly 6-16x V8 time (~1.3-1.9 MB/s), so a multi-megabyte payload
  costs seconds of initial eval. Correctness does not gate the rung;
  interaction latency remains unmeasured.
- **stable-node-wrapper-identity** -- LANDED 2026-07-12. The bridge
  caches one JS wrapper per node keyed by nodeId
  (NODE_WRAPPER_REGISTRY in dom_bridge.rs), so getElementById, the
  createElement result, and the event target share object identity and
  React's fiber/props expandos persist. Measured: the delegated onClick
  now fires on a trusted click and the hooks counter re-renders with
  count 1 (was: handler never called, state stuck at 0). Subsumes
  react-synthetic-event-bridge; details in
  docs/findings/boa-react-bundle-throughput.md.
- element-property-reflection -- frameworks assign el.id and
  textNode.nodeValue as properties; wrapper data properties absorb the
  write without reaching the Dom. Now the sole blocker for a visibly
  updating React counter: the click-driven re-render commits its text
  through a nodeValue assignment the Dom never sees (measured). Back
  reflected properties (nodeValue, data, id, className) with
  write-through accessors (finding follow-up).
- **cdn-challenge-reality-spike** -- TLS fingerprint (JA3/JA4) and
  challenge-JS survival against a Cloudflare-fronted test property;
  rustls default fingerprints may be challenged regardless of engine
  correctness.
- selection-range-ime-editing -- Selection/Range, beforeinput,
  composition events, clipboard; required for composer-class editing
  surfaces (contenteditable editors).
- webcrypto-subtle -- SubtleCrypto digest/HMAC/ECDSA/RSA enough for
  PKCE; required for the live-spa rung's login flows.
- indexeddb-origin-store -- required by production SPA session caches.
- socket-level-streaming-bodies -- incremental chunk delivery from the
  socket inside BasicClient (v1 slices buffered bodies).
- event-loop-waker -- replace the 10 ms in-flight poll with a real
  winit EventLoopProxy wake.
- http2-on-single-request-path -- JS fetch rides HTTP/1.1 today; the
  h2 client serves only batch prefetch.
- dynamic-import / import.meta / top-level-await in test262 scope, and
  the full-corpus re-run (blocked on corpus availability).
- Intl/ICU (AD-021) and FinalizationRegistry host hooks (unchanged).

Deferrals surfaced during the W1-W4 execution (each is a small,
separately-landable follow-up):

- xhr-async-migration -- XMLHttpRequest still runs synchronously inside
  send(); the host-net-completion-queue supports migrating it, but its
  existing tests assert synchronous readyState progression. Migrate the
  object and its tests together.
- fetch-abort-midflight -- AbortSignal is honored at call time only; an
  abort after dispatch does not cancel the worker request.
- popstate-back-forward-dispatch -- pushState entries are recorded in
  session history and the address bar, but back/forward to a
  same-document entry still reloads instead of dispatching popstate
  (needs history-entry-kind metadata on the Vec<String> history).
- change-event-on-blur -- input/keydown/keyup fire; change requires
  focus-time value tracking through clear_page_input_focus.
- ws-es-eventtarget-mixin -- WebSocket/EventSource expose on* handlers
  only; addEventListener on non-DOM targets needs the EventTarget mixin.
- matchmedia-change-events -- matchMedia lists are static snapshots; a
  resize does not fire change events (set_viewport exists; wiring the
  app resize path through re-evaluation remains).
- sse-https -- EventSource speaks plain http:// only; the https path
  should ride BasicClient once socket-level streaming lands there.
- innerhtml-serializing-getter -- innerHTML reads still return
  textContent; a real HTML serializer is needed for the getter.
- open-ws-idle-poll -- an open WebSocket/EventSource holds the 10 ms
  poll cadence; the event-loop waker deferral subsumes this.

## Verification checklist (applies to every workstream)

- make check and make test green with RUSTFLAGS='-D warnings'.
- make full green before any merge-ready claim.
- wpt scorecard: pass count strictly increases when fixtures are
  added; never regresses.
- Behavior-affecting changes carry a bench or probe delta
  (scripts/perf_guardrails.py, make gui-probe).
- Ladder claims only with retained evidence: a scripted load of a
  fixture page exercising the mechanism (click handler mutating DOM,
  innerHTML swap, fetch-then-render, pushState navigation).
- Checks not run are reported as not run with the reason.
