# SilkSurf

SilkSurf is a cleanroom web browser engine written in Rust, targeting a
low-resource, low-latency profile: small event loops, direct pixel
buffers, bounded allocations, and measured hot paths instead of broad
framework surface.

The engine is a 13-crate Cargo workspace plus `silksurf-js` (the
JavaScript runtime, delegating execution to boa_engine via
`SilkContext`). The pipeline: network fetch with TLS policy and response
cache -> html5ever tree construction -> CSS cascade and computed style ->
Taffy flexbox/grid layout -> display-list paint with tiny-skia and SIMD
fills -> native presentation over XCB or winit/softbuffer.

## Workspace map

| Crate | Owns |
|---|---|
| `crates/silksurf-core` | arena allocator, interner, canonical `SilkError` |
| `crates/silksurf-html` | HTML tree construction (html5ever TreeSink) |
| `crates/silksurf-dom` | DOM node/document model, dirty-node tracking |
| `crates/silksurf-css` | CSS tokenize/parse/select, cascade, computed style |
| `crates/silksurf-layout` | Taffy flex/grid layout, inline layout, UAX #9/#14 |
| `crates/silksurf-text` | text measurement and glyph rasterization (cosmic-text) |
| `crates/silksurf-render` | display list, rasterization, SIMD row fills |
| `crates/silksurf-engine` | pipeline orchestration, fused style-layout-paint pass |
| `crates/silksurf-net` | HTTP fetch, redirects, response cache |
| `crates/silksurf-tls` | rustls loader/config surface |
| `crates/silksurf-image` | image decoding |
| `crates/silksurf-gui` | XCB and winit/softbuffer windowing backends |
| `crates/silksurf-app` | browser binary and shell |
| `silksurf-js` | JavaScript runtime (boa_engine; DOM bridge; test262 runner) |

The legacy C tree under `src/`, `include/`, and `CMakeLists.txt` is
retired per AD-024 and removed incrementally;
`docs/LEGACY_C_PORTING.md` maps each C module to its owning crate.

## Build and test

```sh
make check   # fast gate: rustfmt, clippy -D warnings, lint scripts
make test    # workspace tests, warnings denied
make full    # check + test + cargo deny + rustdoc; the merge gate
make bench   # benchmark suite
make gui-probe  # live GUI smoke (needs a Wayland or X11 session)
```

Toolchain: pinned stable Rust (see `rust-toolchain.toml`; MSRV equals
the pinned channel by policy). CI is strict-local-only (AD-009): the
merge gate is `scripts/local_gate.sh full`, wired into git hooks by
`scripts/install-git-hooks.sh`. See `docs/development/LOCAL-GATE.md`.

## Conformance and performance

Current numbers live in `docs/conformance/SCORECARD.md` (every quoted
percentage names its denominator) and `perf/baseline.json` with history
in `perf/history.ndjson`. Headlines at the 2026-07 baselines: the
synthetic WPT harness passes 63/63 fixtures; test262 via the boa runner
passes 99.81% of executed tests, 69.38% of the total suite (Intl,
modules, and async tests are skipped and say so). Fused pipeline
render sits at ~192 us against a 250 us guardrail
(`scripts/perf_guardrails.py`).

## Documentation

- `docs/README.md` -- documentation index
- `docs/ARCHITECTURE.md` -- system design
- `docs/design/ARCHITECTURE-DECISIONS.md` -- ADR ledger (AD-001..AD-024)
- `docs/CLEANROOM.md` -- cleanroom policy: `diff-analysis/` is reference
  analysis only; production code never imports from it
- `docs/TESTING.md`, `docs/PERFORMANCE.md`, `docs/XCB_GUIDE.md`
- `docs/roadmaps/DEBT-RECONCILIATION-ROADMAP.md` -- debt inventory and
  reconciliation plan
- `CONTRIBUTING.md` -- onboarding, gate discipline, hook setup
- `SECURITY.md` -- security policy and reporting

## License

Dual-licensed under MIT (`LICENSE-MIT`) or Apache-2.0
(`LICENSE-APACHE`), at your option.
