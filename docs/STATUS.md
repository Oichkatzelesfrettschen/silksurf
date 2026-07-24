# Current Project Status

> This is the canonical current-state summary for the active Rust tree. It is
> intentionally narrower than roadmap documents and historical completion
> reports. `scripts/check_status_consistency.py` checks the machine-verifiable
> fields against manifests and scorecards.

**Evidence refresh:** 2026-07-23  
**Active branch baseline:** `main` after PR #49  
**Toolchain:** stable Rust 1.94.1, pinned exactly

## Classification

SilkSurf is a functional controlled-content browser prototype and native engine
research platform. It has a real fetch-to-window application path and proven
React-class DOM interaction. It is not yet a production-grade arbitrary-web
browser because the page runtime shares the application process and several
origin, policy, storage, editing, loader, and compatibility surfaces remain
incomplete.

## Active workspace

- 13 workspace members under `crates/`
- one sibling workspace member, `silksurf-js`
- production JavaScript execution: `silksurf-js::SilkContext` wrapping and
  delegating ECMAScript execution to `boa_engine`
- retired hand-written JavaScript VM: historical only under AD-025
- retired C harness: historical only under AD-024

## Integrated application path

The default `silksurf-app` mode opens a winit window and integrates:

1. URL navigation and resource fetch through `silksurf-net`/`silksurf-tls`,
2. html5ever tree construction into the shared SilkSurf DOM,
3. stylesheet collection and CSS cascade,
4. classic and module-script collection within current loader limits,
5. `SilkContext` execution and browser host callbacks,
6. dirty-node tracking and retained/fused style-layout-paint,
7. viewport rasterization and retained native presentation,
8. native pointer/keyboard input dispatch back into page JavaScript.

The event loop is already event-driven: winit waits with `ControlFlow::Wait`,
and `WinitWakeHandle`/the event-loop proxy wakes it for host or navigation work.
There is no current 10 ms GUI polling loop to replace.

## Shell and isolation status

Current shell state is single-view:

- one `BrowserState`,
- one history vector/index,
- one focused page input,
- one optional `BrowserPageRuntime`.

`BrowserPageRuntime` owns the DOM, `SilkContext`, stylesheet/index, fused
workspace/results, display list, image state, and raster scratch in the same
process as browser chrome. Renderer crash/hang isolation, multi-tab state,
profiles, permissions, downloads, and process supervision are open program
items.

## Conformance evidence

### Synthetic WPT-style regression harness

- runner kind: `wpt-synthetic`
- result: **70/70** pass, 0 fail, 0 skip
- scope: in-tree HTML/CSS/layout/paint/JavaScript-event fixtures
- limitation: not the upstream Web Platform Tests corpus; do not quote this as a
  browser interoperability percentage

### test262

Recorded full-corpus baseline from 2026-05-17:

- 33,098 / 33,160 executed = **99.81% of executed tests**
- 33,098 / 47,703 total = **69.38% of the total suite**

The test262 corpus is not vendored. A clean checkout cannot reproduce that
historic full-corpus run without fetching or pinning the corpus, and the latest
JSON artifact may represent a narrower subset. Always state the date, scope,
and both denominators.

### Network protocol harnesses

- TLS loader sanity tests: functional
- h2spec: 0/0 scaffold; no in-tree reproducible server target yet
- HTTP/3: not implemented

## Proven application behavior

Retained findings demonstrate:

- React 18 mount and DOM commit through the `SilkContext` bridge,
- stable per-node wrapper identity for delegated event expandos,
- trusted click dispatch into React handlers,
- state update and text commit into the live DOM,
- running-app retained text repaint at approximately 100 us input-to-present in
  the recorded probe,
- attribute rewrite, keyed reorder, and subtree replacement through fused
  relayout at approximately 190-260 us input-to-present in the recorded probes.

These are evidence for the tested fixtures and hosts, not claims of broad site
compatibility.

## Performance-claim boundary

The 0.01 ms goal means 10 us of **bounded application-owned CPU work** on
selected retained-render paths. It does not include:

- JavaScript parse/evaluation or framework reconciliation,
- network or model response time,
- winit dispatch and compositor scheduling,
- display refresh/scanout,
- arbitrary full-tree layout or repaint.

All performance reports must separate input dispatch, JavaScript/model commit,
layout, raster, frame submission, compositor-visible presentation, and network
or service latency.

## Known public-web blockers

The native engine still lacks or only partially implements:

- renderer/site isolation and a process sandbox,
- complete same-origin enforcement,
- CORS, CSP, SRI, mixed-content and referrer-policy enforcement,
- comprehensive response/script/node/layout/raster/CPU/memory budgets,
- complete parser-blocking/async/defer/module lifecycle semantics,
- import maps, dynamic import, top-level await, workers and service workers,
- complete streaming/cancellation paths,
- Selection/Range/contenteditable/IME/composition,
- WebCrypto `subtle`, IndexedDB, Cache Storage, and other production-SPA APIs.

## Current execution program

GitHub issue #50 and
`docs/roadmaps/BROWSER-FUNCTIONALIZATION-ACTION-PLAN.md` define the program:

1. make status and evidence mechanically consistent,
2. specify and prove a process-neutral engine boundary using the native engine,
3. run comparable WPE/Wry/Servo/CEF integration spikes before fixing a backend
   verdict or crate split,
4. build a multi-view browser shell and persistent profile substrate,
5. build a native virtualized AI-chat mode with bounded mounted state,
6. mature the native engine independently through upstream WPT, loader semantics,
   enforcement, and sandboxing.
