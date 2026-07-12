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
  react-dom was 70-98 ms. A hyperfine sweep tightens these when a
  regression gate needs them.
- The V8 baseline runs in a bare `vm` sandbox (self/window aliases
  only); boa runs with the full SilkContext host surface. The comparison
  bounds engine speed, not host-object overhead.
- The render probe covers mount and one commit; hooks, reconciliation
  under updates, and React's synthetic event delegation (root-installed
  listeners receiving bridge events) are unproven.

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

## Follow-up surface (feeds the deferral wave)

- stable-node-wrapper-identity (ROOT CAUSE, measured): the interactive
  probe (`--click`) shows React committing a hooks component
  (`useState` counter renders) while a trusted click leaves state
  untouched. React locates the fiber for an event by reading the
  expando it stamped on the target DOM node object at commit; the
  bridge builds a fresh wrapper per access, so the stamp lives on a
  dead object and delegation drops the event. The fix is a JS-side
  wrapper cache keyed by nodeId so `getElementById(x)` returns the
  same object across accesses and expando properties persist. This
  subsumes react-synthetic-event-bridge.
- element-property-reflection: React writes `el.id = ...` and
  `textNode.nodeValue = ...` as property assignments; wrapper data
  properties accept the write without reaching the Dom. Audit which
  reflected properties frameworks assign and back them with accessors.
- interaction-latency-probe: measure the keystroke-to-commit path once
  the synthetic-event bridge lands.
