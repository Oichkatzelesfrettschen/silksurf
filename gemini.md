# Gemini Project Summary

## What this repo is
SilkSurf is a cleanroom, Rust-first browser engine stack with modular crates for
HTML, CSS, DOM, layout, rendering, JS runtime, networking, and TLS. The root
also contains legacy C sources kept as reference only.

## Current status (high level)
- Rust core pipeline: HTML -> DOM -> CSS -> layout -> display list -> raster.
- JS engine lives in `silksurf-js` and is integrated via a thin host boundary.
- Performance work is active: arena allocation, interning, fixed-point layout,
  VM hot paths, PGO/BOLT tooling, and riced build profiles.
- CSS cascade now pre-indexed by tag/id/class; selector specificity cached.
- Engine integrates a `StyleCache` (full recompute today; incremental pending).
- Display list tiles support damage-region rasterization; text uses `NodeId` handles.
- Bench pipeline reports stage timings (parse/style/layout/render).

## Known issues / warnings
- `LD_PRELOAD` warnings for `/usr/lib/mklfakeintel.so` appear during builds.
- LBR BOLT profiling failed: PMU does not support branch stack sampling here.

## Key docs
- Architecture: `docs/ARCHITECTURE.md`
- Build and tooling: `docs/BUILD_NOTES.md`, `docs/RUST_TOOLING.md`
- Perf baselines: `docs/PERF_BASELINES.md`
- Engine perf roadmap: `docs/ENGINE_PERF_ROADMAP.md`
- JS perf roadmap: `docs/JS_ENGINE_PERF_ROADMAP.md`
- Installation requirements: `docs/INSTALLATION_REQUIREMENTS.md`
- Warning inventory: `docs/WARNINGS_AUDIT.md`

## Next focus
- Add incremental style invalidation and pre-sorted rule application.
- Tighten perf baselines with real workloads and LBR-enabled BOLT.
- Add perf guardrails for timing/RSS and keep warning-free builds.
