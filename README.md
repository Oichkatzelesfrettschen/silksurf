# SilkSurf

SilkSurf is a cleanroom web browser engine and native browser research platform
written in Rust. It targets a low-resource, low-latency profile: small
work queues, direct pixel buffers, bounded allocations, retained rendering, and
measured hot paths instead of an unbounded framework surface.

The active tree is a 13-crate workspace under `crates/` plus `silksurf-js`.
`silksurf-js` owns the browser host layer and delegates ECMAScript execution to
`boa_engine` through `SilkContext`. The integrated application path is:

```text
network/TLS/cache -> html5ever DOM -> CSS cascade -> Taffy layout ->
JavaScript/DOM mutation -> display list -> tiny-skia/direct-ARGB raster ->
winit presentation on Wayland or X11
```

The windowed winit browser is the default application mode; `--headless` runs a
one-shot static render. An optional legacy XCB backend remains available behind
a feature flag.

## Current product status

SilkSurf is a functional controlled-content browser prototype and engine test
bed. Static pages, external resources, JavaScript, React-class DOM updates,
native input dispatch, incremental repaint, scrolling, navigation, and native
presentation are exercised by in-tree tests and live GUI probes.

It is **not yet a security boundary for arbitrary public-web content**. The
current page runtime is in-process with the shell, and site isolation, CORS,
CSP, SRI, complete origin enforcement, and comprehensive resource budgets are
open work. The current shell is also single-view rather than a complete
multi-tab browser.

See `docs/STATUS.md` for the canonical current-state summary and
`docs/roadmaps/BROWSER-FUNCTIONALIZATION-ACTION-PLAN.md` for the execution
program tracked by GitHub issue #50.

## Workspace map

| Crate | Owns |
|---|---|
| `crates/silksurf-core` | arena allocator, interner, canonical `SilkError` |
| `crates/silksurf-html` | HTML tree construction (html5ever TreeSink) |
| `crates/silksurf-dom` | DOM node/document model, mutation batching, dirty tracking |
| `crates/silksurf-css` | CSS tokenize/parse/select, cascade, computed style |
| `crates/silksurf-layout` | Taffy flex/grid layout, inline layout, UAX #9/#14 |
| `crates/silksurf-text` | text measurement, shaping, glyph rasterization |
| `crates/silksurf-render` | display lists, damage rasterization, direct pixel paths |
| `crates/silksurf-engine` | parse/render orchestration and fused style-layout-paint |
| `crates/silksurf-net` | HTTP fetch, redirects, cookies, response cache, sockets |
| `crates/silksurf-tls` | rustls loader/configuration surface |
| `crates/silksurf-image` | image decoding |
| `crates/silksurf-gui` | winit/Wayland/X11 presentation and optional XCB backend |
| `crates/silksurf-app` | browser application, chrome, navigation, runtime integration |
| `silksurf-js` | `SilkContext`, Boa host integration, DOM/events/net APIs, test262 runner |

The legacy C harness is retired under AD-024. Historical sources and migration
notes remain in git history and `docs/archive/`.

## Build and test

```sh
make check      # rustfmt, clippy -D warnings, policy lints, status consistency
make test       # workspace tests, warnings denied
make full       # check + test + cargo deny + rustdoc; the merge gate
make bench      # benchmark suite
make gui-probe  # live GUI smoke; requires Wayland or X11
```

The repository pins stable Rust 1.94.1 exactly in `rust-toolchain.toml`; the
workspace MSRV moves in lockstep. Routine merge gating is intentionally
local-only under AD-009: `scripts/local_gate.sh full`, normally installed through
`scripts/install-git-hooks.sh`.

## Conformance and performance

Current evidence lives in `docs/conformance/SCORECARD.md`, the per-harness JSON
artifacts, `docs/PERFORMANCE.md`, and `perf/`.

- The in-tree **synthetic** WPT-style regression harness passes **70/70**
  fixtures. It is not the upstream Web Platform Tests corpus and is not a broad
  interoperability percentage.
- The recorded 2026-05-17 test262 full-corpus baseline passes **99.81% of
  executed tests** and **69.38% of the total suite**. The test262 corpus is not
  vendored. That historic full-corpus result is not reproducible from a clean
  checkout until the corpus is fetched or pinned. Newer subset results must not
  be confused with that baseline.
- h2spec remains a 0/0 scaffold pending a reproducible server target.
- The 0.01 ms objective applies only to bounded application-owned CPU work on
  selected retained-render paths. It excludes JavaScript evaluation, network
  time, compositor scheduling, display scanout, and remote-service latency.

## Documentation

- `docs/STATUS.md` -- canonical current status and evidence scope
- `docs/ARCHITECTURE.md` -- current system and process topology
- `docs/JS_ENGINE.md` -- production JavaScript integration and limitations
- `docs/design/ARCHITECTURE-DECISIONS.md` -- ADR ledger
- `docs/roadmaps/BROWSER-FUNCTIONALIZATION-ACTION-PLAN.md` -- browser program
- `docs/roadmaps/DEBT-RECONCILIATION-ROADMAP.md` -- debt inventory
- `docs/CLEANROOM.md` -- cleanroom policy
- `docs/TESTING.md`, `docs/PERFORMANCE.md`, `docs/NETWORK_TLS.md`
- `CONTRIBUTING.md` -- onboarding and gate discipline
- `SECURITY.md` -- security policy and reporting

## License

Dual-licensed under MIT (`LICENSE-MIT`) or Apache-2.0
(`LICENSE-APACHE`), at your option.
