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

- **nested-browsing-context-damage-model** -- OPEN, and it gates every
  HTML 4.8 embedded-content element. `iframe`, `video`, `audio`,
  `object`, `picture`, and `srcset` return zero hits across crates/ and
  silksurf-js/. `iframe` is the decision gate rather than the cheapest
  item: a nested browsing context needs its own document, style tree,
  layout root, and paint subtree, and the shell owns exactly one
  BrowserPageRuntime. The question to answer before any element work is
  whether the retained damage model survives a second document. A nested
  context that forces full-page repaint puts the latency evidence
  (~100 us text repaint, 190-260 us fused relayout) in direct conflict
  with the capability, and that conflict is the finding. Deferred
  because the conformance instrument reached real upstream corpora only
  on 2026-08-06, and admitting embedded-content work before an
  upstream-corpus number moves repeats the synthetic-scorecard failure
  docs/findings/conformance-instrument-fidelity.md records.
- **media-element-stack** -- OPEN. `video` and `audio` need demux,
  decode, and audio output the workspace does not have and does not
  target. `picture` and `srcset` are tractable inside the existing
  crates/silksurf-image surface and are the cheapest real
  embedded-content capability once the damage-model question resolves.

- **boa-bundle-throughput-spike** -- RUN 2026-07-12; verdict and numbers
  in docs/findings/boa-react-bundle-throughput.md. React 18 mounts and
  commits into the silksurf DOM (after ownerDocument and DOM-interface
  constructor stubs landed); boa evaluates minified framework code at
  roughly 6-16x V8 time (~1.3-1.9 MB/s), so a multi-megabyte payload
  costs seconds of initial eval. Correctness does not gate the rung.
  Interaction latency MEASURED 2026-07-16: 100 dispatch-to-commit
  cycles at p50 0.76 ms / p95 1.15 ms over the bridge, insensitive to
  host load (eval is the memory-bound phase; the commit path is not);
  docs/findings/react-interaction-commit-latency.md carries the
  distribution, methodology, and retained CSV data.
- **stable-node-wrapper-identity** -- LANDED 2026-07-12. The bridge
  caches one JS wrapper per node keyed by nodeId
  (NODE_WRAPPER_REGISTRY in dom_bridge.rs), so getElementById, the
  createElement result, and the event target share object identity and
  React's fiber/props expandos persist. Measured: the delegated onClick
  now fires on a trusted click and the hooks counter re-renders with
  count 1 (was: handler never called, state stuck at 0). Subsumes
  react-synthetic-event-bridge; details in
  docs/findings/boa-react-bundle-throughput.md.
- **element-property-reflection** -- LANDED 2026-07-12. nodeValue, data,
  id, and className are live wrapper accessors: nodeValue/data write
  through Dom::set_text_content (React commits text by assigning them),
  id/className read and write the id/class attributes. React routes
  className/id through setAttribute (no property assignment in the
  bundle), so text was the sole gap. Measured over the JS/DOM bridge:
  the full --click inc probe drives the counter to a committed clicks:1
  in document.body.textContent (was clicks:0). Also erases the read
  staleness stable-node-wrapper-identity introduced on cached wrappers.
  Running-app repaint CLOSED 2026-07-17
  (docs/findings/local-spa-click-repaint-gui-probe.md): make
  gui-probe-page-click synthesizes a trusted PrimaryClick into the live
  Wayland surface; dispatch_native_click runs the page's JS click
  handler, the counter text mutation takes the retained repaint path,
  and the app presents a Damage(Rect) frame at input_to_present ~100 us.
  The GUI input-synthesis -> dirty-node paint -> present class is now
  proven, not just the bridge Dom mutation. Fused-relayout reconcile
  CLOSED 2026-07-18
  (docs/findings/local-spa-fused-reconcile-gui-probe.md): make
  gui-probe-attr-reconcile, gui-probe-reorder-reconcile, and
  gui-probe-subtree-reconcile drive an attribute rewrite, a keyed list
  reorder, and a subtree replace. Each escapes the retained text-only fast
  path, so repaint_runtime_dirty_nodes reruns fused style/layout/paint and
  presents mode Full at input_to_present ~190-260 us. A Runtime fused
  repaint: dirty_nodes=N trace names the branch (N = 1, 3, 9); the probe
  asserts it fires while the text-only marker does not.
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
- svg-paint-pipeline -- silksurf-render, silksurf-image, and
  silksurf-layout carry no SVG handling, so an `<svg>` subtree paints
  nothing. chatgpt.com draws its logo and every icon as inline SVG, and
  the shell renders with blank gaps where they sit. A real
  implementation needs the SVG document structure, path geometry,
  fill and stroke, transforms, gradients, `viewBox` mapping, and
  `<svg>` sized as a replaced element in layout. `usvg` plus `resvg`
  would supply it against the tiny-skia backend silksurf-render already
  rasterizes through, at a dependency weight the low-resource profile
  has not accepted.
- mutation-observer -- MutationObserver is undefined. `Dom::take_dirty_nodes`
  already records the mutated set the fused pipeline consumes, so the
  records exist; delivering them needs a per-observer subtree filter and
  a microtask-checkpoint queue.
- dynamic-import-fetch -- the module graph is fetched ahead of
  evaluation from the static imports boa reports, so `import()` reaches
  a module the registry does not hold and rejects. chatgpt.com's entry
  module route-splits through `import()`, so its page code never
  evaluates. Wiring it needs load_imported_module to fetch on demand
  rather than report a miss.
- intl-formatters -- Intl carries Locale and getCanonicalLocales, which
  is what language negotiation reads. DateTimeFormat, NumberFormat,
  Collator, PluralRules, and RelativeTimeFormat stay absent rather than
  wrong: a formatter that ignores the locale produces text a page
  presents as localized. boa_engine's `intl` feature supplies
  spec-correct implementations backed by icu4x, at a binary-size and
  compile-time cost the low-resource profile has not accepted.

## Live document resources (landed 2026-08-19)

The pipeline treated the document's resources as a parse-time snapshot; three
mechanisms now re-collect from the DOM. AD-028 records the decision and
AD-029 the cascade half.

- Stylesheets are a live ordered list (`StyleSheetSet`), re-collected when the
  tree shape moves or a dirty node is a `<style>` or `<link>`, and reparsed
  into `StyleIndex` when it changes.
- `<link rel=preload>` fetches and fires `load` or `error` at its element
  (`PreloadLinks`), which is what a startup script waits on before upgrading a
  link to a stylesheet.
- IDL attribute reflection lives on the interface prototypes, so
  `element.rel`, `img.src`, `input.disabled`, and the rest of the HTML table
  read and write the content attribute.
- `document.currentScript` names the `<script>` element under evaluation.
- `var()` resolves in the cascade against an inherited, Arc-shared map, and
  the `background`, `font`, `place-items`, and `inset` shorthands plus the CSS
  logical box properties reach the computed style.
- `--screenshot PATH` writes the headless frame as PNG, which is what turns a
  rendering claim into a file a reviewer opens.

## Positioned-box rendering (landed 2026-08-19)

- A `position: fixed` subtree lays out under a second taffy root sized to the
  viewport (`TaffyLayout::viewport_root`), so its insets and percentages
  resolve against the viewport per CSS Position 3 2.1. An axis whose two
  insets both compute to auto keeps the CSS static position.
- The paint pass walks the stacking-context tree (`build_paint_order`): the
  context element's box, then negative z-index children, then in-flow members
  in tree order, then zero and positive z-index children, recursively. A
  z-index-3 pane paints its background before the subtree it contains.
- `display: none` suppresses the boxes of the whole subtree
  (`mark_rendered_boxes`), not just the declaring element's own box.
- `--monitor <selector>` binds the window to one monitor, matching the
  connector name the display server reports and the EDID Display Product Name
  from DRM sysfs, so both `DP-2` and `LG` reach the same panel.
  `--list-monitors` prints both names. Wayland names an output only through
  `xdg_toplevel.set_fullscreen`, so a named monitor opens borderless
  fullscreen there; X11 positions the window at the monitor's origin.
  `SILKSURF_MONITOR` supplies the default, which keeps a host's connector
  names out of the repository.

## Application-shell paint fixture (landed 2026-08-19)

`crates/silksurf-engine/conformance/wpt/fixtures/css_spa_shell_stacking.html`
is a synthetic application shell carrying the three structures a
client-rendered page depends on: a `position: fixed` root with `inset: 0`, a
`z-index: 3` pane holding its own text, and a `display: none` subtree whose
descendants carry text. Its document order runs pane, watermark, flow while
its paint order runs watermark, flow, pane, so a BFS paint order fails the
fixture rather than passing it by coincidence.

The specimen is synthetic because a captured chatgpt.com document is
third-party markup that a live fetch of a logged-in origin can carry account
state into, which is what keeps `silksurf-extras/` and `silksurf-js/test262/`
untracked. Reproducing the structures gives the same discrimination with
bytes the repository owns.

`crates/silksurf-engine/tests/spa_shell_render.rs` asserts the same three
invariants over the same file, so `make test` covers what `make conformance`
scores; `make full` runs the workspace tests and not the conformance harness,
so a fixture reachable only from the catalog would not run before a push.
Each assertion was falsified by reverting its mechanism: suppressing only the
declaring element's box surfaces "consent banner" in the paint list, painting
in BFS order gives pane=5 against flow=9, and resolving the fixed insets
against the document root gives `#shell` a 76.8 px height at y=38.4.
Scorecard 70/70 to 71/71.

## Absolute containing block (landed 2026-08-19)

`position: absolute` mapped to taffy's `Position::Absolute`, which resolves
against the taffy parent, so a box whose DOM parent is static took that
parent's origin and size rather than the nearest positioned ancestor's. CSS
Position 3 2.1 names the nearest ancestor whose position is not static, and
the initial containing block when no ancestor qualifies.

`ContainingBlock` now records which block owns each box, `assign_placements`
resolves it in one forward BFS pass, and `group_adopted_by_ancestor` groups
the reparented boxes so `rebuild` reads one contiguous run per taffy node it
builds. A reparented box becomes a taffy child of that block; a box whose DOM
parent already is the block keeps taffy's own placement unchanged. An absolute
box with no positioned ancestor joins `viewport_root`, which carries the
initial containing block and the viewport alike until scrolling separates them.

The share of absolute boxes this moves, counted per document: chatgpt.com 5 of
20, en.wikipedia.org 5 of 8, github.com 16 of 19, example.com 0 of 0. Against
the before-and-after headless renders that is 1591 differing pixels on
chatgpt.com and 356094 on github.com, whose skip link moves to the initial
containing block origin; a mid-page crop is pixel-identical, so the difference
localizes to the positioned boxes.

Reparenting empties a wrapper whose only child was absolute, and the measure
closure gave a childless element a one-line auto height. CSS 2.1 10.6.3 gives
a block box with no in-flow line box a height of zero, so
`generates_no_line_box` marks any element that has children and returns zero
from the closure. A genuinely childless element keeps the existing floor,
which is recorded as the `empty-block-line-height-floor` cut.

## Window-width reflow (landed 2026-08-19)

`FRAME_WIDTH` was the row stride at every site that read or wrote the page
bitmap, so the document laid out at 1280 px whatever the window presented.
`BrowserFrame::raster_width` now carries it and the layout viewport is the
window surface below the browser chrome (`browser_layout_viewport`), which is
what a rotated or non-1280 output needs: `--monitor LG` fullscreens onto a
90-degree-rotated panel and gets a 1440x3440 surface.

A page builds against the live window size when one exists
(`build_browser_page_with_buffers_for_window`), and
`reflow_browser_page_for_window` compares the runtime's viewport against the
surface on every frame and relayouts when they disagree. That comparison is
what carries the compositor's answer: `WinitWindow::new` requests a size and
`--monitor` fullscreen overrides it, so the size the window opens at arrives
through `WindowEvent::Resized` after the first page is already built.

`reflow_runtime_for_viewport` rebuilds `StyleIndex` before the fused pipeline
reruns, because `StyleIndex::for_viewport` evaluates the media queries when it
flattens the active rules; a relayout without that rebuild moves the geometry
and leaves every breakpoint-dependent declaration on the previous branch.
`set_viewport` follows the relayout, so matchMedia answers the size the
document is laid out at. matchMedia lists stay static snapshots, so a script
that already ran keeps its earlier answer -- that half stays the
matchmedia-change-events deferral.

Three retained mechanisms cached the old width and each needed the stride in
its key. `refresh_browser_frame_bitmap` compared scroll offset and row count
only, so a pure-width resize hit its early return and presented the previous
width's bitmap; `bitmap_raster_width` joins the key and the scroll-reuse path,
which shifts rows at one stride, refuses a width that moved.
`FocusViewportCache` and `ScrollViewportCache` are bitmaps the window presents
word for word, so both record the width they were rastered at and a reflow
drops them. `FusedWorkspace` gated its taffy rebuild on
`Dom::structure_generation` and `style_generation` alone, and neither moves
when the viewport does: the cascade resolves `vw`, `vh`, and the `@media`
branch against the viewport, so the retained taffy styles held the previous
width's `ComputedStyle` while the fresh cascade output sat unused. The rebuild
now fires on a viewport that moved as well.

`MAX_SCREENSHOT_HEIGHT` did its 5-MiB arithmetic against 1280, so it became
`MAX_SCREENSHOT_PIXELS` and a document divides it by its own raster width.

`crates/silksurf-app/src/window_frame.rs` asserts the discrimination against a
fixture whose `#box` is 300 px wide only above a 1000 px viewport and whose
`#fluid` is `50vw`: a reflow that relayouts without rebuilding `StyleIndex`
moves `#fluid` and leaves `#box` wide. Each mechanism was falsified by
reverting it -- dropping the taffy viewport gate, the `StyleIndex` rebuild, and
the stride from the bitmap key each fail their own assertion. Measured with
`make gui-probe-page-click` over three runs: render min 11.2 to 11.5 us
against a 11.3 us baseline, and `make gui-probe-attr-reconcile` min 40.7 to
42.5 us against 52.2 us.

## Important-declaration layer inversion (landed 2026-08-19)

`Specificity::cascade_key` takes the importance bit and returns the layer rank
or its complement, which is what CSS Cascade 5, 6.4.4 asks for: an important
declaration in an earlier layer beats one in a later layer, and an unlayered
important declaration loses to every layered one. The complement carries both,
because `UNLAYERED` is `u32::MAX` and complements to the minimum while the
first layer's rank 0 complements to the maximum.

`Specificity` gains an `element_attached` field ahead of `layer`, so the
`style` attribute keeps the element-attached step of CSS Cascade 5, 6.4.3 in
both importance classes rather than riding the selector counts of `u32::MAX`
it previously carried. Leaving it inside the reversal would have made an
important layered rule beat an important `style` attribute.

`ResolvedProperty::should_override` and `custom_property_wins` both read the
key. Five cases in `crates/silksurf-css/tests/conditional_rules.rs` mirror the
normal-declaration ordering the existing layer tests fix, so a comparison that
ignores importance fails one set or the other.

Closing this surfaced a separate defect the same tests exposed: an inline
`!important` written without a trailing semicolon parsed as a normal
declaration, because `CssTokenizer::finish` appends `CssToken::Eof` and
`parse_declarations` carried it into the value where `consume_important`
reads. Landed separately.

## Import-map scopes (landed 2026-08-19)

`silksurf_js::ImportMap` carries both members of the document's import map.
`PageModuleLoader::apply_import_map` takes the referrer's URL and consults the
scopes whose prefix it matches, longest prefix first, before falling through to
the top-level `imports`; applying a scoped mapping to every referrer resolves
the specifier to the wrong module, which is why the referrer reaches the lookup
rather than the specifier alone.

`set_import_map` resolves each scope key against the document's address, which
is what HTML 8.1.3.8 asks of a scope key before it is compared to a referrer, and
drops a key that does not resolve because it names no referrer.
`document_import_map` reads both members out of the `<script type=importmap>`
element.

Seven cases pair a referrer inside a scope with one outside it, so a lookup
that applies the scope to every referrer passes the first and fails the second.

## Stale-entry revalidation (landed 2026-08-19)

`ResponseCache::get` answers only while an entry is fresh, and a stale entry
stayed in the map carrying its ETag and Last-Modified with nothing consulting
them, so a stale navigation refetched the whole body.
`SpeculativeRenderer::fetch_or_speculate` now sends
`ResponseCache::conditional_headers` on the miss path (RFC 9111, 4.3.1); an
absent entry offers none and the request stays a plain GET.

`ResponseCache::refresh_from_not_modified` turns the origin's 304 into the
stored representation (RFC 9111, 4.3.4): the 304's headers update the stored
ones, the freshness window restarts from now because the origin has just
spoken about the entry, and the body, status, and the validators the 304 omits
stay. `FetchOrigin::Revalidated` names the outcome, so the navigation trace
separates one round-trip with no body bytes from a full refetch.

Four cases in `crates/silksurf-net/src/cache.rs`. Dropping the freshness
restart fails `a_not_modified_answer_serves_the_stored_body`.

## Registered custom properties (landed 2026-08-19)

`@property` parsed structurally and its descriptors were discarded, so a
`var(--unset)` with no fallback substituted to nothing and left its declaration
unapplied. `PropertyRegistration` now reads `syntax`, `inherits`, and
`initial-value` out of the at-rule's declaration block, and `collect_active_rules`
collects the registrations on the same walk that flattens the rules, so a
registration inside `@media`, `@supports`, or `@layer` registers alongside the
rules that block admits. CSS Properties and Values 1, 2 makes `syntax` and
`inherits` required and `initial-value` required for every syntax but the
universal `*`, so a rule missing one registers nothing.

`apply_registrations` puts each initial value into the element's map before its
own declarations are recorded, which is what makes a registered name answer a
`var()` at every element that neither declares nor inherits it. The
registrations are a per-element floor rather than a root seed because the
document node carries a default `ComputedStyle`, so no element ever cascades
with `parent: None`. A registration with `inherits: false` overwrites the
inherited value, which is how the parent's declaration stops at the child; an
inheriting one fills only a name the map does not already hold. The map is
rebuilt only when a registration is unsatisfied, so the parent's `Arc` stays
shared below the element that established the values.

The syntax string is retained without being enforced: a registration's
observable effect here is the initial value and the inheritance the flag turns
off. Type-checking a declared value against its registered syntax is named as
the `registered-property-syntax-enforcement` cut below.

Eight cases in `crates/silksurf-css/tests/conditional_rules.rs`, including the
pair that differs only in the `inherits` flag. Dropping `apply_registrations`
fails three of them; making every registration inherit fails
`a_non_inheriting_registration_stops_at_the_child`.

## Document stylesheets and the CSSOM (landed 2026-08-20)

`document.styleSheets`, `CSSStyleSheet`, and `HTMLStyleElement.sheet` were
undefined. Emotion, which styles chatgpt.com, reaches its sheet through
`if (e.sheet) return e.sheet; for (var t = 0; t < document.styleSheets.length; t++)`
and the accessor call sits outside the `try` guarding its `insertRule`, so the
undefined list raised a TypeError out of the page's own style path and every
component rule was lost. Seven of the 1163 chunks reachable from
`/cdn/assets/manifest-56d12409.js` touch the CSSOM; Emotion is the one that
paints. AD-030 carries the design.

`silksurf_css::SheetSet` keeps one `Stylesheet` per source beside the owner
node, href, and media it carries, and answers `insert_rule`, `delete_rule`, and
`set_disabled` by index. `StyleIndex::for_viewport_sheets` flattens the set's
active sheets in list order, which is what walking one concatenation already
produced, and one `LayerOrder` spans the list because CSS Cascade 5, 6.4.4
gives a layer name one rank per document. `serialize.rs` turns a parsed rule
back into text for `cssText`, `selectorText`, and `CSSStyleDeclaration.cssText`,
which the crate could not do in any form.

`SheetSet::script_generation` is the whole signal a splice leaves: an
`insertRule` call touches no DOM node, so neither `Dom::structure_generation`
nor `Dom::generation` reports it and `StyleSheetSet::refresh` sees nothing.
`drain_scripted_stylesheets` compares the generation the current `StyleIndex`
was built from, which costs one integer comparison on a tick that spliced
nothing. The set is built before the document's scripts run, so a startup
script reaches `document.styleSheets` at the point Emotion asks for it.

`install_computed_style_provider` took a `Stylesheet` cloned once at page build
and was called once, so `getComputedStyle` answered from a parse-time snapshot
and observed neither a CSSOM splice nor the AD-028 rebuild. It now shares the
handle the rebuild writes.

Two defects surfaced while testing this. `FusedWorkspace` rebuilt its retained
taffy styles on a DOM generation or a viewport change, and a CSSOM splice moves
neither, so `StyleIndex` carries a monotonic `build_id` the gate compares.
`CascadeWorkspace::prepare` grew `matched_by_rule` with `Vec::resize`, which
leaves the existing elements alone, so a rule list that gained an entry carried
the previous run's match cache into indices the new index had reassigned --
`html` painted with the declarations of a rule selecting `#box`.

Eight cases in `silksurf-js/tests/style_sheets.rs`, including Emotion's
accessor and insert in the shape the bundle ships, eight in
`crates/silksurf-css/tests/cssom_sheet_set.rs`, nine in
`crates/silksurf-css/tests/rule_serialization.rs` including a round-trip
through the parser, and two end-to-end cases in `crates/silksurf-app`.
Removing `set_style_sheets` reproduces the original TypeError; reverting the
`matched_by_rule` clear reproduces the misattribution.

## Open work after the live-resource change

Named cuts, each with the mechanism that closes it:

- css-transform-beyond-translation -- `transform` contributes its translation
  component to the paint rect; rotate, scale, skew, and matrix contribute
  nothing, because every DisplayItem is an axis-aligned rect. Carrying them
  needs a transform per display item and a rasterizer that applies it to
  geometry and to shaped glyph runs.
- chrome-width-responsive -- the address bar is `ADDRESS_BAR_X` 108 plus
  `ADDRESS_BAR_WIDTH` 880, so it ends at 988 px whatever the window is. A
  window wider than that leaves the remainder bare and a narrower one clips
  the bar, because `fill_argb_rect` bounds every write to the surface. Closing
  it makes the bar's width the surface minus the button strip and the right
  margin.
- registered-property-syntax-enforcement -- `PropertyRegistration` retains the
  `syntax` descriptor without checking a declared value against it, so a
  declaration whose value does not match the registered grammar applies rather
  than falling back to the initial value. CSS Properties and Values 1, 5 makes
  a mismatched value invalid at computed-value time. Closing it needs the
  syntax string parsed into a component-value grammar the cascade can match a
  token list against.
- empty-block-line-height-floor -- the measure closure in
  `TaffyLayout::compute` ends by giving a childless element an auto height of
  one line box. CSS 2.1 10.6.3 gives a block-level non-replaced in-flow box
  with `height: auto` and no in-flow line box a height of zero. An element
  with children reaches the zero already, through `generates_no_line_box`; a
  genuinely childless one keeps the 16 px floor, and removing it moves in-flow
  geometry on every page carrying an empty element.
- fixed-position-scrolling -- a `position: fixed` box now resolves against the
  viewport, and the static render has one scroll origin, so nothing yet holds
  it still while the page scrolls. The windowed browser scrolls by offsetting
  the paint rect, so closing it exempts the viewport-anchored subtrees from
  that offset.
- z-index-auto-context-escape -- `ComputedStyle::z_index` resolves `auto` to 0,
  so `build_paint_order` treats every positioned element as establishing a
  stacking context. CSS 2.1 Appendix E lets a positioned z-auto element's
  positioned descendants join the ancestor context instead. Closing it needs
  `z-index` to carry `auto` distinctly through the cascade.
- viewport-units-in-calc -- `parse_length` matches `Function("calc")`, consumes
  balanced component values, and stores the resulting `CalcExpr` in the
  computed-style arena. `CascadedStyle::resolve` normalizes em/rem and
  viewport units, while layout supplies the percentage basis for the retained
  tree.
- constructed-stylesheets-and-adoption -- `new CSSStyleSheet`, `replaceSync`,
  and `adoptedStyleSheets` stay undefined by AD-030. Their only observed
  consumer attaches a shadow root, and CodeMirror's StyleModule gates its
  working `<style>` text fallback on `adoptedStyleSheets` being absent.
- cssom-synchronous-restyle -- an `insertRule` followed by `getComputedStyle`
  in the same script observes the pre-insert cascade, because AD-030 rebuilds
  `StyleIndex` on the repaint tick rather than inside the JS call.
- cssom-grouping-rules -- `CSSMediaRule` and `CSSSupportsRule` carry no
  `parentRule` or `parentStyleSheet` walk. The session recorder in
  `2340486e-eab5bn2wcgxcv5rd.js` reads them to emit mutation records and
  paints nothing.
- backdrop-filter-and-mask -- `backdrop-filter`, `mask-image`, and
  `mask-composite` parse to nothing; the paint list carries no filter or mask
  stage.
- animation-and-transition -- the `animation-*` and `transition` longhands
  parse to nothing and `@keyframes` blocks flatten out of the cascade, so an
  element animated into view keeps its start state.

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
