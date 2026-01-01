# Rust Crate Audit (Performance + Roles)

This audit summarizes each Rust crate’s role, key characteristics, and where
performance-focused dependencies are already in use.

## Workspace Root
- `Cargo.toml`: centralizes versions to reduce build drift and keep perf
  tooling consistent across crates.

## Core Engine Crates
- `crates/silksurf-core`: arena allocator (`bumpalo`) and string interning
  (`lasso`) plus small-string storage (`smol_str`). These back DOM, CSS, and
  layout allocations.
- `crates/silksurf-dom`: storage/traversal only. Element/attribute names use
  enums + small strings, and id/class values are selectively interned for fast
  comparisons.
- `crates/silksurf-html`: tokenizer + tree builder. Uses `memchr` to accelerate
  byte scanning. Additional wins likely from arena-backed token buffers.
- `crates/silksurf-css`: tokenizer/parser/selectors/cascade. Matching now uses
  tag/attribute enums + `SelectorIdent` (optional atoms) to reduce string churn;
  further gains likely from `phf` keyword tables.
- `crates/silksurf-layout`: block/inline layout tree backed by `SilkArena`
  allocations and arena-backed child lists; layout uses fixed-point width
  calculations for stable positioning.
- `crates/silksurf-render`: display list + raster. SIMD pixel ops and damage
  tracking are still pending.
- `crates/silksurf-engine`: orchestration pipeline. Uses `HashMap` for computed
  style lookup; consider stable interning + compact IDs for cache-friendly maps.
- `crates/silksurf-net` / `crates/silksurf-tls`: TLS via `rustls` (safe, fast).

## JS Engine
- `silksurf-js`: largest performance focus. Uses `memchr` (SIMD scans),
  `bumpalo` (arena GC), `lasso` (interning), `zerocopy` + `rkyv` (zero-copy
  bytecode), `bytemuck` (NaN-boxing), `phf` (keyword lookup).

## Actionable Performance Alignment
- Integrate `silksurf-core` arena + interner into DOM/CSS/layout.
- Apply fast keyword lookup in CSS (e.g., `phf` or interned identifiers).
- Introduce SIMD pixel ops + damage tracking in `silksurf-render`.
- Keep the JS host boundary ID-based to avoid large allocations/copies.
