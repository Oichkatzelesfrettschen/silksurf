# boa React-Bundle Throughput and Correctness

**Date**: 2026-07-12
**Mechanism**: `silksurf-js/src/bin/bundle_probe.rs` evaluates production
UMD bundles in a SilkContext (fresh or shared, stub or live-DOM), times
combined parse+execute, runs a correctness expression, and optionally
pumps host callbacks so scheduler-deferred work commits.
**Question**: does boa 0.21 block the SPA-capability ladder's react-class
rung on correctness or on throughput?

## Verdict

Correctness does not block the rung. React 18.3.1 (react +
react-dom production UMD) loads, initializes, mounts through
`ReactDOM.createRoot`, and **commits a rendered tree into the silksurf
DOM** through the bridge: after the render probe,
`document.body.textContent` carries the component output. Reaching that
point surfaced two bridge gaps, both fixed in the same change:

1. Element wrappers lacked `ownerDocument`; react-dom's listener install
   walks `container.ownerDocument` before any other node API.
2. The realm lacked DOM interface constructors; react-dom's selection
   restore evaluates `node instanceof win.HTMLIFrameElement`, which
   throws when the right-hand side is undefined. The live-document
   bootstrap now defines constructor stubs (Node, Element, HTMLElement,
   HTMLIFrameElement, ...); wrappers are plain objects, so instanceof
   correctly reports false.

Throughput is a UX pressure, not a wall. Release-build eval times on
this host (AMD, cachyos), against node/V8 on identical sources:

| Bundle | bytes | boa eval | V8 eval | ratio |
|---|---|---|---|---|
| react 18.3.1 UMD | 10 751 | 4.0-5.8 ms | 0.8 ms | ~6x |
| moment 2.30.1 | 58 890 | 23.0 ms | 3.5 ms | ~7x |
| lodash 4.17.21 | 73 015 | 56.4 ms | 9.0 ms | ~6x |
| react-dom 18.3.1 UMD | 131 835 | 70-98 ms | 6.2 ms | ~11-16x |

boa evaluates minified framework code at roughly 1.3-1.9 MB/s. A
chatgpt.com-class payload (3-5 MB of JS) extrapolates to roughly 2-4
seconds of initial script evaluation -- tolerable for first paint,
hostile for time-to-interactive. Per-interaction re-render cost (the
keystroke -> setState -> commit path) is unmeasured and is the next
throughput question.

## Falsifiers and scope

- Timings are single-run wall clock on one host; run-to-run spread on
  react-dom was 70-98 ms. A 20-run sweep with order statistics ran
  2026-07-16 under a documented busy host
  (docs/findings/react-interaction-commit-latency.md, Data): boa
  medians there run 2.5-3x these values while V8 moved far less, so
  treat this table as the quiet-host reference and the sweep as the
  busy-host bound plus distribution data.
- The V8 baseline runs in a bare `vm` sandbox (self/window aliases
  only); boa runs with the full SilkContext host surface. The comparison
  bounds engine speed, not host-object overhead.
- The render probe covers mount, one commit, a delegated click, and one
  state-driven re-render. Larger reconciliation under repeated updates, and
  the DOM-visible commit of that re-render (blocked on
  element-property-reflection), remain unproven.

## Reproducer

```sh
cargo build --release -p silksurf-js --bin bundle_probe
# fetch react/react-dom/lodash/moment UMD bundles from unpkg, then:
target/release/bundle_probe --shared --dom --pump \
  react.production.min.js react-dom.production.min.js \
  'render_probe.js=if (document.body.textContent.indexOf("hello-from-react") < 0) throw 0'
```

Bundle hashes at measurement time (sha256):
`d949f1...` react 18.3.1, `35f4f9...` react-dom 18.3.1,
`a9705d...` lodash 4.17.21, `845c52...` moment 2.30.1 (full hashes in
the session evidence; bundles are not vendored -- no network in tests).

## React event delegation lands on stable wrapper identity

The bridge caches one JS wrapper per node keyed by `nodeId`
(`NODE_WRAPPER_REGISTRY` in `dom_bridge.rs`), so `getElementById(x)`, the
`createElement` result, and the event target all resolve to the same object.
React stamps `__reactFiber$<key>` and `__reactProps$<key>` on the host node at
commit and reads them back at dispatch; a fresh wrapper per access stranded
those expandos on a dead object and delegation dropped every event.

Measured against the `--click inc` counter probe (React 18.3.1 `useState`
counter, `createRoot(document.body)`, delegated `onClick`):

- The button node carries both expandos after commit
  (`__reactFiber$egh376mbn1n`, `__reactProps$egh376mbn1n`), reachable through
  `getElementById('inc')`.
- The delegated click listener registers on the root container
  (`__silksurfListenerTypeCounts.click == 2`: one bubble, one capture on the
  body node).
- A trusted click at the button fires the delegated handler
  (`onClick` runs) and advances state: `Counter` re-renders with
  `count == 1` (render count goes 1 -> 2).

Counterfactual (same corrected probe, wrapper cache reverted to the
pre-cache `dom_bridge.rs`): the trusted click leaves `onClickFired == 0` and
the component at its initial render. Reverting only the cache isolates it as
the cause; the probe's click-check fix is held constant across both runs.

## element-property-reflection commits the re-render to the Dom

Grepping the react-dom bundle for the properties React assigns during commit
shows `.nodeValue=` (2 sites) and `.data=` (3 sites) for text, and zero
`.className=`/`.id=` property assignments -- React routes `className`/`id`
through `setAttribute`, which the bridge already writes through. So the text
commit was the sole gap: `nodeValue`, `data`, `id`, and `className` are now
live accessors on the wrapper. `nodeValue`/`data` read the node's current
character data and write through `Dom::set_text_content` (rewrites the Text
node in place and marks it dirty); `id`/`className` read and write the `id`
and `class` attributes. Converting `id`/`className`/`nodeValue` from static
snapshot properties to accessors also erases the read-staleness the wrapper
cache introduced -- a cached wrapper now reflects later `setAttribute` writes.

With this in place the full `--click inc` probe passes: a trusted click drives
the counter to a committed `clicks:1` in `document.body.textContent` (was
`clicks:0`, the re-render's text never reaching Dom state). This is a
JS/DOM-bridge result: the probe drives a synthetic Dom, synthesizes the click
through `dispatch_dom_event`, and reads committed Dom text. Mount, delegated
event, hooks state, re-render, and the Dom-visible commit all work over the
bridge.

The running-app path is a separate, still-unproven evidence class: network
fetch, HTML parse, the GUI input synthesis in `silksurf-app/src/js_events.rs`,
dirty-node paint, and native present are not exercised by the probe.
Substantiating the local-spa rung means loading a counter page through the
real app (`make gui-probe`) and confirming the click repaints, not just that
Dom text mutates.

## Follow-up surface (feeds the deferral wave)

- local-spa-rung-gui-probe: load a React counter page through the running app
  and confirm the click drives a visible repaint, closing the gap between the
  bridge result and the ladder's local-spa acceptance.
- interaction-latency-probe: RUN 2026-07-16. bundle_probe --click-repeat
  times 100 dispatch-to-commit cycles at p50 0.76 ms / p95 1.15 ms;
  verdict, methodology, and retained data in
  docs/findings/react-interaction-commit-latency.md.
- broader reconciliation: the probe covers one state-driven commit; list
  reordering with keys, attribute-only updates, and unmount/remount under
  repeated updates are unproven.
