# Cleanroom Policy and Sources

SilkSurf is a cleanroom implementation. Reference repositories are used only to
distill behaviors into specs and tests; no code is copied.

## Reference Sources (Local Checkouts)
- `silksurf-extras/Amaya-Editor`: layout/editor UI ideas and rendering behavior.
- `silksurf-extras/boa`: JS engine architecture patterns.
- `silksurf-extras/servo`: HTML/CSS/DOM/layout architecture patterns.
- `silksurf-js/test262`: JavaScript conformance tests.

## Distillation Workflow
1. Read source material and write a spec in `silksurf-specification/`.
2. Record invariants/behavioral rules and expected inputs/outputs.
3. Derive tests from the spec (no code reuse).
4. Implement Rust modules guided by the spec + tests only.

## Intake Log
Record every source review and the derived artifacts in the template below.
See `docs/archive/cleanroom/CLEANROOM_INTAKE_LOG.md` for the full log history.

Template:
- Date:
- Source (repo/path/doc):
- Area (HTML/CSS/JS/DOM/Layout/Net/TLS/GUI):
- Summary (behavioral notes only):
- Distilled specs (silksurf-specification/*.md):
- Tests added (crate/test path):
- Implementation notes (design constraints, perf targets):

## Legacy C Porting Map (Summary)
C sources are retained for cleanroom reference only. Rust is the build target.

Document + DOM:
- HTML tokenizer/tree builder -> `crates/silksurf-html` (implemented core paths).
- DOM nodes/attributes -> `crates/silksurf-dom` (Tag/Attribute enums, interning).
- CSS selector handler -> `crates/silksurf-css` (SelectorIdent + interner fast paths).

CSS:
- Tokenizer/parser/selectors -> `crates/silksurf-css` (implemented core paths).
- Cascade/computed values -> `crates/silksurf-css` (partial).

Layout + Render:
- Layout tree -> `crates/silksurf-layout` (arena-backed + fixed-point).
- Render list/raster -> `crates/silksurf-render` (partial; SIMD path in place).

GUI + Platform:
- XCB wrappers/event loop -> `crates/silksurf-gui` (pending).

JS Runtime:
- Rust-first runtime in `silksurf-js`, host bridge in `crates/silksurf-engine/src/js.rs`.

For full per-file mapping, see `docs/archive/cleanroom/LEGACY_C_PORTING.md`.
