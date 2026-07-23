# JavaScript Runtime Guide

## Production runtime

`silksurf-js` is the browser host-integration crate. Production ECMAScript
execution is delegated to `boa_engine`; `SilkContext` owns the Boa context and
installs SilkSurf's DOM, event, scheduling, networking, storage, crypto, and
browser-global surfaces.

The former hand-written lexer/parser/bytecode VM, GC, and JIT are not the
production runtime. They were removed under AD-025 and survive only in git
history and archived design material. Performance plans for that retired VM
must not be presented as current work.

## Application integration

The current windowed application integrates JavaScript directly rather than
through a fully abstract engine-owned runtime:

1. HTML parsing constructs a `silksurf_dom::Dom`.
2. `silksurf-app` wraps that DOM in `Arc<Mutex<Dom>>`.
3. `SilkContext` is created over the same live DOM and shared cookie/storage
   context.
4. classic scripts and currently supported module graphs are evaluated.
5. Boa jobs and SilkSurf host callbacks are drained by the application/event
   loop.
6. host and framework mutations mark DOM nodes dirty.
7. the application chooses retained text repaint or fused style/layout/paint and
   submits native damage.

`crates/silksurf-engine/src/js.rs` contains a runtime abstraction/stub surface,
but it is not the complete path used by `silksurf-app`. Treating that file as
the integration source of truth produces stale architecture conclusions.

## Current host surface

The active Boa backend includes, at varying completeness:

- live DOM construction/query/mutation wrappers,
- stable wrapper identity by `NodeId`,
- capture/target/bubble event dispatch and native-input event synthesis,
- timers, requestAnimationFrame, microtasks, and host-callback scheduling,
- `fetch` completion queues and buffered `ReadableStream` delivery,
- persistent WebSocket sessions and EventSource support,
- cookies shared with the network client,
- origin-keyed local storage persistence,
- history intents, `matchMedia`, and `getComputedStyle`,
- browser globals, console, randomness, and basic crypto helpers.

The exact API contract belongs in source-level tests and the synthetic
JavaScript fixtures; this guide intentionally does not claim full Web IDL or web
platform coverage.

## Known gaps affecting production SPAs

Important open or partial surfaces include:

- complete script ordering (`async`, `defer`, parser blocking) and document
  lifecycle events,
- import maps, dynamic import, `import.meta`, top-level await, and complete
  module graph semantics,
- socket-level streaming and complete abort/cancellation behavior,
- asynchronous XHR migration and full EventTarget behavior on non-DOM targets,
- workers, service workers, Cache Storage, and IndexedDB,
- WebCrypto `subtle`, Intl/ICU, and FinalizationRegistry integration,
- Selection/Range, `beforeinput`, contenteditable, clipboard, IME, and
  composition,
- complete origin/CORS/CSP/SRI enforcement and page resource budgets.

See `docs/roadmaps/SPA-CAPABILITY-ROADMAP.md`, `docs/STATUS.md`, and the browser
functionalization action plan for ownership and sequencing.

## Conformance evidence

`silksurf-js/src/bin/test262_boa.rs` evaluates test262 through Boa and reports
both executed-test and total-suite denominators.

The recorded 2026-05-17 full-corpus baseline is:

- 33,098 / 33,160 executed = 99.81%
- 33,098 / 47,703 total = 69.38%

The test262 corpus is not vendored. That historic full-corpus result is not
reproducible from a clean checkout until the corpus is fetched or pinned, and
the latest JSON artifact may contain a narrower subset run. Always report date,
scope, skips, and both denominators.

The engine's 70/70 WPT-labelled result is a synthetic in-tree browser regression
suite, not upstream WPT interoperability evidence.

## Measured performance boundary

Retained findings distinguish startup/evaluation from steady interaction:

- production minified framework evaluation in Boa is materially slower than V8
  on the measured host and can dominate time-to-interactive,
- a small React counter's dispatch-to-DOM-commit path measured p50 0.76 ms,
  p95 1.15 ms, and p99 2.41 ms over 100 asserted commits,
- live retained text repaint and broader fused reconciliation are separate
  application/presenter evidence classes.

Do not combine JavaScript commit, layout, paint, frame submission, compositor
presentation, and network/service latency into one number.

## Current performance priorities

1. macro-profile real application startup and interaction before micro-tuning,
2. reduce initial bundle parse/evaluation pressure or select a compatibility
   backend for public-web mode,
3. keep host-object identity and mutation paths allocation-aware,
4. bound host callback work and page resource consumption,
5. preserve retained repaint paths for the native engine and native-chat UI,
6. publish distributions and retained fixtures for every performance claim.

## Feature flags and tools

The current `silksurf-js` package keeps default features minimal. Optional
features cover allocator, CLI, and structured tracing support. The primary
binaries are:

- `test262_boa` -- test262 evaluator/scorecard tool,
- `bundle_probe` -- production-bundle correctness and latency probe,
- `silksurf` -- optional CLI entry point.

See `silksurf-js/Cargo.toml`, `docs/findings/`, and
`docs/conformance/SCORECARD.md` for the current executable truth.
