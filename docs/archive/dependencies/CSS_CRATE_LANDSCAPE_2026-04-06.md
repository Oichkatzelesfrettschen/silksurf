# CSS Crate Landscape Analysis (Pure Rust) — 2026-04-06

## Goal and scope
- Surveyed a broad Rust crate landscape for CSS engine-adjacent needs in SilkSurf: parser/tokenizer, selector matching, style/cascade representation, interning/small-string, perf helpers, serialization/cache, and compliance tooling.
- Explicitly prioritized pure-Rust crates, maintenance signal, architecture fit, and cleanroom boundary safety.

## Discovery method (evidence)
1. **crates.io search sweep** using queries:
   - `css parser`
   - `css selector`
   - `css tokenizer`
   - `css cascade`
   - `css style`
   - `string interner`
   - `small string`
   - `arena allocator`
   - `simd utf8`
   - `serde cache`
   - `zero copy serialization`
   - `wpt css`
   - `fuzz css`
2. **Ecosystem discovery** via dependency expansion from key CSS crates:
   - `lightningcss@1.0.0-alpha.71` → `cssparser`, `cssparser-color`, `parcel_selectors`, `smallvec`, `indexmap`
   - `selectors@0.36.1` → `cssparser`, `phf`, `precomputed-hash`, `servo_arc`, `smallvec`, `rustc-hash`
   - `swc_css_parser@21.0.0` → `swc_css_ast`, `swc_css_visit`, `serde`
   - `parcel_selectors@0.28.2` → `cssparser`, `phf`, `precomputed-hash`, `smallvec`
   - `biome_css_parser@0.5.8` → `biome_parser`, `biome_rowan`, `insta`
3. **Per-candidate metadata pull** from crates.io API (version, update recency, download signals).
4. **Quick-fit scoring** for each candidate: maintenance, API fit, performance potential, overlap risk, cleanroom compatibility.

## Candidate set summary
- Total candidates evaluated: **50**
- Tier counts: **adopt now=17**, **evaluate/prototype=25**, **avoid/reject=8**
- Area coverage:
  - parser tokenizer: **10**
  - selector matching: **7**
  - style representation: **2**
  - interning small string: **8**
  - perf helpers: **9**
  - serialization cache: **8**
  - spec compliance tooling: **6**

## Ranked recommendation tiers

### 1) Adopt now (aligned + low overlap risk)
Best immediate-fit crates are mostly focused utilities and testing/compliance tooling that strengthen SilkSurf without replacing core cleanroom-owned CSS logic.

| crate | area | maintenance signal | API fit | perf potential | overlap risk | cleanroom compat | rationale |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `phf` | selector matching | high (2025-08-23, 70901709 recent dl) | high | medium | low | high | Compile-time keyword tables align with parser hot paths and deterministic lookup. |
| `csscolorparser` | style representation | high (2026-03-09, 3574282 recent dl) | high | medium | low | high | Small focused color parser fits typed color pipeline with minimal overlap. |
| `smol_str` | interning small string | high (2026-03-04, 15761894 recent dl) | high | high | low | high | Already integrated and aligned with short CSS identifier workload. |
| `bumpalo` | perf helpers | high (2026-02-19, 73079776 recent dl) | high | high | low | high | Already core arena allocator; proven fit for parse/build phases. |
| `indexmap` | perf helpers | high (2026-04-02, 186664983 recent dl) | high | high | low | high | Deterministic hash map iteration useful for stable cascade/debug output. |
| `memchr` | perf helpers | high (2026-02-06, 161062560 recent dl) | high | high | low | high | Already core SIMD scanning primitive for tokenizer hot loops. |
| `rustc-hash` | perf helpers | high (2026-03-28, 101947005 recent dl) | high | high | low | high | Already used fast hash implementation, good fit for style indexes. |
| `simdutf8` | perf helpers | medium (2024-09-22, 28647624 recent dl) | high | high | low | high | Fast UTF-8 validation; candidate for tokenizer ingress fast-path. |
| `smallvec` | perf helpers | high (2025-11-16, 133639083 recent dl) | high | high | low | high | Already core stack-first vector optimization in selector/style structures. |
| `serde` | serialization cache | high (2025-09-27, 149035257 recent dl) | high | medium | low | high | Foundation for fixtures and serializable style/cache structures. |
| `serde_json` | serialization cache | high (2026-01-06, 145247153 recent dl) | high | medium | low | high | Current safe default for stylesheet cache serialization path. |
| `arbitrary` | spec compliance tooling | high (2025-08-14, 20917025 recent dl) | high | medium | low | high | Bridges corpus generation to fuzz/property tests. |
| `datatest-stable` | spec compliance tooling | high (2026-03-31, 582059 recent dl) | high | medium | low | high | Good fit for WPT-style fixture sweeps and golden conformance runs. |
| `insta` | spec compliance tooling | high (2026-03-30, 14626011 recent dl) | high | medium | low | high | Snapshot testing accelerates parser/serializer regression triage. |
| `libfuzzer-sys` | spec compliance tooling | high (2026-02-10, 9252642 recent dl) | high | medium | low | high | Fuzzing backbone for parser/tokenizer hardening. |
| `proptest` | spec compliance tooling | high (2026-03-24, 25479094 recent dl) | high | medium | low | high | Property-based testing aligns with parser/cascade invariants. |
| `similar-asserts` | spec compliance tooling | high (2026-04-01, 5535088 recent dl) | high | medium | low | high | Better textual diffs for expected-vs-actual CSS outputs. |

### 2) Evaluate / prototype (high potential, moderate integration risk)
These crates are promising but should go through bounded prototype spikes (bench + conformance + memory profile) before adoption decisions.

| crate | area | maintenance signal | API fit | perf potential | overlap risk | cleanroom compat | rationale |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `cssparser` | parser tokenizer | high (2026-03-17, 10649044 recent dl) | medium | high | medium | medium | Mature low-level token/grammar parser; best as differential oracle, not full replacement. |
| `cssparser-macros` | parser tokenizer | high (2026-03-17, 8791426 recent dl) | medium | medium | medium | medium | Useful only if cssparser chosen for grammar helpers. |
| `swc_css_ast` | parser tokenizer | high (2026-04-03, 78430 recent dl) | medium | medium | medium | medium | AST model for swc_css_parser; useful only in differential harnesses. |
| `swc_css_parser` | parser tokenizer | high (2026-04-03, 74794 recent dl) | medium | high | medium | medium | Fast parser with broad syntax coverage; useful for corpus differential testing. |
| `swc_css_visit` | parser tokenizer | high (2026-04-03, 76907 recent dl) | medium | medium | medium | medium | Visitor tooling for swc AST; harness-only utility. |
| `ego-tree` | selector matching | high (2026-01-23, 3834788 recent dl) | medium | medium | medium | medium | Useful tree model for fixture/harness-level selector differential checks. |
| `html5ever` | selector matching | high (2026-03-13, 13127596 recent dl) | medium | medium | medium | medium | Strong spec corpus and parser infra; suitable as behavior reference harness. |
| `markup5ever` | selector matching | high (2026-03-13, 13213598 recent dl) | medium | medium | medium | medium | Needed companion types if html5ever-based harnessing is used. |
| `selectors` | selector matching | high (2026-03-18, 9020499 recent dl) | medium | high | medium | medium | Spec-aligned selector engine; strong conformance oracle, high runtime overlap with in-house matcher. |
| `tendril` | selector matching | high (2026-01-09, 10411172 recent dl) | medium | medium | medium | medium | Efficient string buffer abstraction if html5ever harness path is taken. |
| `cssparser-color` | style representation | high (2026-03-17, 534188 recent dl) | medium | medium | medium | medium | Fine-grained CSS Color syntax helper; test for value model fit first. |
| `arcstr` | interning small string | medium (2024-05-07, 3012235 recent dl) | medium | high | medium | medium | Arc-based immutable string; good sharing semantics, likely higher overhead for hot parser paths. |
| `compact_str` | interning small string | medium (2025-02-25, 25430167 recent dl) | medium | high | medium | medium | Potential smol_str alternative with different allocation behavior; benchmark first. |
| `intaglio` | interning small string | high (2026-03-30, 943870 recent dl) | medium | high | medium | medium | Promising modern interner candidate; worth side-by-side microbench. |
| `internment` | interning small string | medium (2024-10-12, 2416065 recent dl) | medium | high | medium | medium | Good for static interned values; weaker fit for mutable parse-time interning. |
| `kstring` | interning small string | medium (2024-07-25, 8363208 recent dl) | medium | high | medium | medium | Useful for mixed borrowed/owned string APIs; evaluate if API ergonomics demand it. |
| `lasso` | interning small string | medium (2024-08-19, 1162903 recent dl) | medium | high | medium | medium | Previously used in this repo; now removed to avoid transitive duplication, so re-adopt only with measured win over local interner. |
| `string-interner` | interning small string | medium (2025-02-11, 6106332 recent dl) | medium | high | medium | medium | Viable alternative interner; benchmark against lasso before any switch. |
| `ahash` | perf helpers | high (2025-05-08, 90603574 recent dl) | medium | high | medium | medium | Very fast hashing but non-determinism/DoS trade-offs need explicit policy. |
| `id-arena` | perf helpers | high (2026-01-14, 16971458 recent dl) | medium | high | medium | medium | Simple typed ID arena; useful where stable IDs matter more than raw speed. |
| `slotmap` | perf helpers | high (2025-12-06, 15192891 recent dl) | medium | high | medium | medium | Robust stable key container; evaluate for DOM/style graph mutation-heavy paths. |
| `bitcode` | serialization cache | high (2025-12-18, 2098017 recent dl) | medium | high | medium | medium | Compact binary serde-compatible format; benchmark versus serde_json/rkyv. |
| `bytemuck` | serialization cache | high (2026-01-31, 45427031 recent dl) | medium | high | medium | medium | Useful for POD casts in hot paths when layout invariants are explicit. |
| `rkyv` | serialization cache | high (2026-02-10, 24348964 recent dl) | medium | high | medium | medium | High-performance zero-copy option; requires careful versioning and ABI discipline. |
| `zerocopy` | serialization cache | high (2026-03-28, 134106202 recent dl) | medium | high | medium | medium | Strong primitive for safe binary layouts and cache transport. |

### 3) Avoid / reject (misaligned or policy risk)
These are currently poor fits due to cleanroom overlap, architecture mismatch, immature ecosystem signal, or prior policy/advisory concerns.

| crate | area | maintenance signal | API fit | perf potential | overlap risk | cleanroom compat | rationale |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `biome_css_parser` | parser tokenizer | medium (2024-12-18, 1851 recent dl) | low | medium | high | low | Biome ecosystem dependency chain is heavy for this use-case. |
| `css_lexer` | parser tokenizer | high (2026-04-06, 3645 recent dl) | low | medium | high | low | Very small ecosystem and unclear maintenance depth versus mature alternatives. |
| `lightningcss` | parser tokenizer | high (2026-03-09, 504262 recent dl) | low | medium | high | low | Excellent performance but full engine (parser/transform/minify); too much cleanroom overlap. |
| `swc_css_compat` | parser tokenizer | high (2026-04-03, 53325 recent dl) | low | medium | high | low | Build-time compatibility transform layer, not browser runtime need. |
| `swc_css_modules` | parser tokenizer | high (2026-04-03, 26418 recent dl) | low | medium | high | low | CSS Modules transform crate; not aligned with browser engine architecture. |
| `parcel_selectors` | selector matching | high (2025-05-11, 498655 recent dl) | low | medium | high | low | Parcel-specific selector stack; overlaps selectors + adds bundler coupling. |
| `bincode` | serialization cache | high (2025-12-16, 39331361 recent dl) | low | medium | high | low | Prior repo debt includes RustSec unmaintained finding; keep out until policy re-evaluated. |
| `postcard` | serialization cache | high (2025-07-24, 7830925 recent dl) | low | medium | high | low | Previously triggered transitive unmaintained advisory path in this repo context. |

## Optimized shortlist for SilkSurf (priority order)
1. **Conformance/tooling now**: `datatest-stable`, `proptest`, `arbitrary`, `libfuzzer-sys`, `insta`, `similar-asserts`.
2. **Parser/index perf now**: keep `smallvec`, `memchr`, `rustc-hash`, `smol_str`; add `phf` and prototype `simdutf8` in tokenizer ingress path.
3. **Color pipeline now**: adopt `csscolorparser` for typed color parsing while keeping tokenization/parsing ownership in `silksurf-css`.
4. **Targeted prototypes**:
   - Differential parser/selector harness with `cssparser` + `selectors` and optionally `swc_css_parser` as cross-check oracle.
   - Interner A/B microbench: current local interner baseline vs `intaglio` vs `string-interner` (optionally include `lasso` as an oracle).
   - Cache format spike: `serde_json` baseline vs `rkyv`/`bitcode`/`zerocopy`+`bytemuck` under strict compatibility constraints.

## Cleanroom alignment notes
- Keep **runtime CSS parser/matcher ownership** in `crates/silksurf-css`; use external parser/matcher crates primarily as test oracles and corpus differentials.
- Reject full-engine replacements (`lightningcss`) and build-tool-centric transforms (`swc_css_modules`, `swc_css_compat`) for runtime integration.
- Preserve deterministic behavior and reproducible fixtures by preferring stable iteration/container choices (`indexmap` when needed) and explicit serialization contracts.

## Evidence artifacts
- Raw metadata: `docs/archive/dependencies/CSS_CRATE_LANDSCAPE_2026-04-06_METADATA.csv`
- Evaluated dataset: `docs/archive/dependencies/CSS_CRATE_LANDSCAPE_2026-04-06_EVALUATED.csv`
