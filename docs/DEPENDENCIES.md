# Dependency Policy and Audit

This document merges dependency rationale, utilization audits, and crate role
summaries. Detailed raw scans live in `docs/archive/dependencies/`.

## Principles
- Prefer small, focused crates (parsing, encoding, data structures).
- Avoid full engines or frameworks that break cleanroom boundaries.
- Document why each dependency exists and where it is used.
- Keep licenses MIT/Apache-2.0 unless explicitly reviewed.

## Workspace Dependencies (Current)
Core utilities used across crates:
- `bumpalo`: arena allocation (`silksurf-core`, `silksurf-js`).
- `lasso`: string interning (`silksurf-core`, `silksurf-js`).
- `memchr`: SIMD byte scans (`silksurf-html`, `silksurf-js`).
- `smol_str`: small-string storage (`silksurf-core`).
- `thiserror`: error types (`silksurf-core`).
- `serde`/`serde_json`: test fixtures (`silksurf-html` tests).
- `rustls`: TLS (`crates/silksurf-tls`).

## Crate Roles (Performance Focus)
- `silksurf-core`: arena allocator + interner + small strings.
- `silksurf-dom`: nodes/attributes with enums + selective interning.
- `silksurf-html`: tokenizer + tree builder, memchr hot paths.
- `silksurf-css`: tokenizer/parser/selectors/cascade; tag/id/class indexing.
- `silksurf-layout`: arena-backed layout tree, fixed-point metrics.
- `silksurf-render`: display list + raster; SIMD row fill in place.
- `silksurf-engine`: orchestration; incremental style/layout wiring.
- `silksurf-net`/`silksurf-tls`: TLS via rustls.
- `silksurf-js`: JS runtime (memchr, bumpalo, lasso, rkyv/zerocopy, bytemuck).

## Support Crates (HTML/CSS Helpers)
Optional/reference helpers (use only if they do not replace cleanroom logic):
- `html5ever`, `markup5ever`, `tendril`: tokenizer/tree-building references.
- `cssparser`, `selectors`: CSS syntax and selector references.
- `encoding_rs`: encoding tables for HTML input streams.

## Audit Notes
- Dependency usage is tracked per crate; see
  `docs/archive/dependencies/DEPENDENCY_UTILIZATION.md`.
- Macro/derive use is tracked separately (JS engine); see
  `docs/archive/js/JS_ENGINE_DEP_USAGE.md`.
