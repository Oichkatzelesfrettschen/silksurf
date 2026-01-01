# Dependency Utilization Audit (First-Party)

Heuristic scan for `crate::` and `extern crate` paths. Macro-only usage may be missed.
Generated: 2026-01-01 03:48 UTC

## Audit Notes (Feature Flags + Macros)
- Only `silksurf-js` declares feature flags; see `docs/JS_ENGINE_FEATURE_AUDIT.md`.
- Known derive/proc-macro usage may not show as `dep::` (e.g., `thiserror`, `bytemuck`).
- Workspace dependencies are listed for reference; usage is tracked per crate section below.

## workspace-root
Path: `Cargo.toml`

### workspace-dependencies
- bumpalo (workspace-dependencies; features: collections; default-features: true; optional: false; uses: none)
- lasso (workspace-dependencies; features: none; default-features: true; optional: false; uses: none)
- memchr (workspace-dependencies; features: none; default-features: true; optional: false; uses: none)
- rustls (workspace-dependencies; features: none; default-features: true; optional: false; uses: none)
- serde (workspace-dependencies; features: derive; default-features: true; optional: false; uses: none)
- serde_json (workspace-dependencies; features: none; default-features: true; optional: false; uses: none)
- smol_str (workspace-dependencies; features: none; default-features: true; optional: false; uses: none)
- thiserror (workspace-dependencies; features: none; default-features: true; optional: false; uses: none)

### workspace-dependency usage map (heuristic)
- bumpalo: declared by [silksurf-core, silksurf-js]; used by [silksurf-core, silksurf-js]
- lasso: declared by [silksurf-core, silksurf-js]; used by [silksurf-core, silksurf-js]
- memchr: declared by [silksurf-html, silksurf-js]; used by [silksurf-html, silksurf-js]
- rustls: declared by [silksurf-tls]; used by [silksurf-tls]
- serde: declared by [silksurf-html]; used by [none]
- serde_json: declared by [silksurf-html]; used by [silksurf-html]
- smol_str: declared by [silksurf-core]; used by [silksurf-core]
- thiserror: declared by [silksurf-core]; used by [silksurf-core]

## silksurf-app
Path: `crates/silksurf-app`

### dependencies
- silksurf-engine (dependencies; features: none; default-features: true; optional: false; uses: src)

## silksurf-core
Path: `crates/silksurf-core`

### dependencies
- bumpalo (dependencies; features: none; default-features: true; optional: false; uses: src)
- lasso (dependencies; features: none; default-features: true; optional: false; uses: src)
- smol_str (dependencies; features: none; default-features: true; optional: false; uses: src)
- thiserror (dependencies; features: none; default-features: true; optional: false; uses: src)

## silksurf-css
Path: `crates/silksurf-css`

### dependencies
- silksurf-core (dependencies; features: none; default-features: true; optional: false; uses: src)
- silksurf-dom (dependencies; features: none; default-features: true; optional: false; uses: src, tests)

## silksurf-dom
Path: `crates/silksurf-dom`

### dependencies
- silksurf-core (dependencies; features: none; default-features: true; optional: false; uses: src)

## silksurf-engine
Path: `crates/silksurf-engine`

### dependencies
- silksurf-core (dependencies; features: none; default-features: true; optional: false; uses: src, tests)
- silksurf-css (dependencies; features: none; default-features: true; optional: false; uses: src, tests)
- silksurf-dom (dependencies; features: none; default-features: true; optional: false; uses: src, tests)
- silksurf-gui (dependencies; features: none; default-features: true; optional: true; uses: none)
- silksurf-html (dependencies; features: none; default-features: true; optional: false; uses: src)
- silksurf-js (dependencies; features: none; default-features: true; optional: true; uses: none)
- silksurf-layout (dependencies; features: none; default-features: true; optional: false; uses: src, tests)
- silksurf-net (dependencies; features: none; default-features: true; optional: true; uses: none)
- silksurf-render (dependencies; features: none; default-features: true; optional: false; uses: src)
- silksurf-tls (dependencies; features: none; default-features: true; optional: true; uses: none)

### Unused (heuristic)
silksurf-gui (dependencies), silksurf-js (dependencies), silksurf-net (dependencies), silksurf-tls (dependencies)

## silksurf-gui
Path: `crates/silksurf-gui`

## silksurf-html
Path: `crates/silksurf-html`

### dependencies
- memchr (dependencies; features: none; default-features: true; optional: false; uses: src)
- silksurf-dom (dependencies; features: none; default-features: true; optional: false; uses: src, tests)

### dev-dependencies
- serde (dev-dependencies; features: none; default-features: true; optional: false; uses: none)
- serde_json (dev-dependencies; features: none; default-features: true; optional: false; uses: tests)

### Unused (heuristic)
serde (dev-dependencies)

## silksurf-layout
Path: `crates/silksurf-layout`

### dependencies
- silksurf-core (dependencies; features: none; default-features: true; optional: false; uses: src, tests)
- silksurf-css (dependencies; features: none; default-features: true; optional: false; uses: src, tests)
- silksurf-dom (dependencies; features: none; default-features: true; optional: false; uses: src, tests)

## silksurf-net
Path: `crates/silksurf-net`

### dependencies
- silksurf-tls (dependencies; features: none; default-features: true; optional: false; uses: src)

## silksurf-render
Path: `crates/silksurf-render`

### dependencies
- silksurf-css (dependencies; features: none; default-features: true; optional: false; uses: src, tests)
- silksurf-dom (dependencies; features: none; default-features: true; optional: false; uses: src, tests)
- silksurf-layout (dependencies; features: none; default-features: true; optional: false; uses: src, tests)

### dev-dependencies
- silksurf-core (dev-dependencies; features: none; default-features: true; optional: false; uses: tests)

## silksurf-tls
Path: `crates/silksurf-tls`

### dependencies
- rustls (dependencies; features: none; default-features: true; optional: false; uses: src)

## silksurf-js
Path: `silksurf-js`

### dependencies
- bitvec (dependencies; features: none; default-features: true; optional: false; uses: src)
- bumpalo (dependencies; features: collections; default-features: true; optional: false; uses: src)
- bytemuck (dependencies; features: derive; default-features: true; optional: false; uses: src)
- clap (dependencies; features: derive; default-features: true; optional: true; uses: src)
- console_error_panic_hook (dependencies; features: none; default-features: true; optional: true; uses: src)
- cranelift-codegen (dependencies; features: none; default-features: true; optional: true; uses: src)
- cranelift-frontend (dependencies; features: none; default-features: true; optional: true; uses: src)
- cranelift-jit (dependencies; features: none; default-features: true; optional: true; uses: src)
- cranelift-module (dependencies; features: none; default-features: true; optional: true; uses: src)
- cranelift-native (dependencies; features: none; default-features: true; optional: true; uses: src)
- lasso (dependencies; features: none; default-features: true; optional: false; uses: src)
- memchr (dependencies; features: none; default-features: true; optional: false; uses: src)
- memmap2 (dependencies; features: none; default-features: true; optional: true; uses: src)
- mimalloc (dependencies; features: none; default-features: true; optional: true; uses: src)
- napi (dependencies; features: napi4; default-features: true; optional: true; uses: src)
- napi-derive (dependencies; features: none; default-features: true; optional: true; uses: src)
- phf (dependencies; features: macros; default-features: true; optional: false; uses: src)
- rkyv (dependencies; features: none; default-features: true; optional: false; uses: src)
- static_assertions (dependencies; features: none; default-features: true; optional: false; uses: src)
- tracing (dependencies; features: none; default-features: true; optional: true; uses: src)
- tracing-subscriber (dependencies; features: env-filter; default-features: true; optional: true; uses: src)
- unicode-xid (dependencies; features: none; default-features: true; optional: false; uses: src)
- wasm-bindgen (dependencies; features: none; default-features: true; optional: true; uses: src)
- zerocopy (dependencies; features: derive; default-features: true; optional: false; uses: src)

### dev-dependencies
- criterion (dev-dependencies; features: html_reports; default-features: true; optional: false; uses: benches)
- proptest (dev-dependencies; features: none; default-features: true; optional: false; uses: none)
- tracing-test (dev-dependencies; features: none; default-features: true; optional: false; uses: none)

### build-dependencies
- phf_codegen (build-dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
proptest (dev-dependencies), tracing-test (dev-dependencies), phf_codegen (build-dependencies)
