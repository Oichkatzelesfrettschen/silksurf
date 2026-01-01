# Dependency Utilization Audit (silksurf-extras)

Heuristic scan for `crate::` and `extern crate` paths. Macro-only usage may be missed.
Generated: 2025-12-31 23:11 UTC

## Audit Notes
- `silksurf-extras/*` is reference-only and not compiled by the workspace.
- Feature/macro usage is heuristic; validate against each crate’s `Cargo.toml`.

## boa_benches
Path: `silksurf-extras/boa/benches`

### dependencies
- boa_engine (dependencies; features: none; default-features: true; optional: false; uses: benches)
- boa_runtime (dependencies; features: none; default-features: true; optional: false; uses: benches)

### dev-dependencies
- criterion (dev-dependencies; features: none; default-features: true; optional: false; uses: benches)
- walkdir (dev-dependencies; features: none; default-features: true; optional: false; uses: benches)

### target.x86_64-unknown-linux-gnu.dev-dependencies
- jemallocator (target.x86_64-unknown-linux-gnu.dev-dependencies; features: none; default-features: true; optional: false; uses: benches)

## boa_cli
Path: `silksurf-extras/boa/cli`

### dependencies
- boa_engine (dependencies; features: deser, flowgraph, trace; default-features: true; optional: false; uses: src)
- boa_gc (dependencies; features: none; default-features: true; optional: false; uses: src)
- boa_parser (dependencies; features: none; default-features: true; optional: false; uses: src)
- boa_runtime (dependencies; features: none; default-features: true; optional: false; uses: src)
- clap (dependencies; features: derive; default-features: true; optional: false; uses: src)
- color-eyre (dependencies; features: none; default-features: true; optional: false; uses: src)
- colored (dependencies; features: none; default-features: true; optional: false; uses: src)
- cow-utils (dependencies; features: none; default-features: true; optional: false; uses: src)
- dhat (dependencies; features: none; default-features: true; optional: true; uses: src)
- futures-concurrency (dependencies; features: none; default-features: true; optional: false; uses: src)
- futures-lite (dependencies; features: none; default-features: true; optional: false; uses: src)
- phf (dependencies; features: macros; default-features: true; optional: false; uses: src)
- regex (dependencies; features: none; default-features: true; optional: false; uses: src)
- rustyline (dependencies; features: derive, with-file-history; default-features: true; optional: false; uses: src)
- serde_json (dependencies; features: none; default-features: true; optional: false; uses: src)

### target.cfg(target_os = "windows").dependencies
- mimalloc-safe (target.cfg(target_os = "windows").dependencies; features: skip_collect_on_exit; default-features: true; optional: true; uses: src)

### target.x86_64-unknown-linux-gnu.dependencies
- jemallocator (target.x86_64-unknown-linux-gnu.dependencies; features: none; default-features: true; optional: true; uses: src)

## boa_ast
Path: `silksurf-extras/boa/core/ast`

### dependencies
- arbitrary (dependencies; features: derive; default-features: true; optional: true; uses: src)
- bitflags (dependencies; features: none; default-features: true; optional: false; uses: src)
- boa_interner (dependencies; features: none; default-features: true; optional: false; uses: src, tests)
- boa_macros (dependencies; features: none; default-features: true; optional: false; uses: src)
- boa_string (dependencies; features: none; default-features: true; optional: false; uses: src, tests)
- indexmap (dependencies; features: none; default-features: true; optional: false; uses: src)
- num-bigint (dependencies; features: none; default-features: true; optional: false; uses: src)
- rustc-hash (dependencies; features: std; default-features: true; optional: false; uses: src)
- serde (dependencies; features: derive; default-features: true; optional: true; uses: src)

## boa_engine
Path: `silksurf-extras/boa/core/engine`

### dependencies
- aligned-vec (dependencies; features: none; default-features: true; optional: false; uses: src)
- arrayvec (dependencies; features: none; default-features: true; optional: false; uses: none)
- bitflags (dependencies; features: none; default-features: true; optional: false; uses: src)
- boa_ast (dependencies; features: none; default-features: true; optional: false; uses: src)
- boa_gc (dependencies; features: thin-vec, boa_string; default-features: true; optional: false; uses: src)
- boa_icu_provider (dependencies; features: std; default-features: true; optional: true; uses: src)
- boa_interner (dependencies; features: none; default-features: true; optional: false; uses: src)
- boa_macros (dependencies; features: none; default-features: true; optional: false; uses: src)
- boa_parser (dependencies; features: none; default-features: true; optional: false; uses: src, tests)
- boa_string (dependencies; features: none; default-features: true; optional: false; uses: src, tests)
- bytemuck (dependencies; features: derive; default-features: true; optional: false; uses: src)
- cfg-if (dependencies; features: none; default-features: true; optional: false; uses: src)
- cow-utils (dependencies; features: none; default-features: true; optional: false; uses: src)
- dashmap (dependencies; features: none; default-features: true; optional: false; uses: src)
- dynify (dependencies; features: none; default-features: true; optional: false; uses: src)
- either (dependencies; features: none; default-features: true; optional: true; uses: src)
- fast-float2 (dependencies; features: none; default-features: true; optional: false; uses: src)
- fixed_decimal (dependencies; features: ryu; default-features: true; optional: true; uses: src)
- float16 (dependencies; features: none; default-features: true; optional: true; uses: src)
- futures-channel (dependencies; features: none; default-features: true; optional: false; uses: src)
- futures-concurrency (dependencies; features: none; default-features: true; optional: false; uses: src)
- futures-lite (dependencies; features: none; default-features: true; optional: false; uses: src)
- hashbrown (dependencies; features: none; default-features: true; optional: false; uses: src)
- iana-time-zone (dependencies; features: none; default-features: true; optional: true; uses: src)
- icu_calendar (dependencies; features: none; default-features: false; optional: true; uses: src)
- icu_casemap (dependencies; features: serde; default-features: false; optional: true; uses: src)
- icu_collator (dependencies; features: serde; default-features: false; optional: true; uses: src)
- icu_datetime (dependencies; features: serde; default-features: false; optional: true; uses: src)
- icu_decimal (dependencies; features: serde; default-features: false; optional: true; uses: src)
- icu_list (dependencies; features: serde, alloc; default-features: false; optional: true; uses: src)
- icu_locale (dependencies; features: serde; default-features: false; optional: true; uses: src)
- icu_normalizer (dependencies; features: compiled_data, utf16_iter; default-features: true; optional: false; uses: src)
- icu_plurals (dependencies; features: serde, experimental; default-features: false; optional: true; uses: src)
- icu_provider (dependencies; features: none; default-features: true; optional: true; uses: src)
- icu_segmenter (dependencies; features: auto, serde; default-features: false; optional: true; uses: src)
- indexmap (dependencies; features: std; default-features: true; optional: false; uses: src)
- intrusive-collections (dependencies; features: none; default-features: true; optional: false; uses: src)
- itertools (dependencies; features: none; default-features: false; optional: false; uses: src)
- lz4_flex (dependencies; features: none; default-features: true; optional: true; uses: src)
- num-bigint (dependencies; features: serde; default-features: true; optional: false; uses: src)
- num-integer (dependencies; features: none; default-features: true; optional: false; uses: src)
- num-traits (dependencies; features: none; default-features: true; optional: false; uses: src)
- num_enum (dependencies; features: none; default-features: true; optional: false; uses: src)
- paste (dependencies; features: none; default-features: true; optional: false; uses: src)
- portable-atomic (dependencies; features: none; default-features: true; optional: false; uses: src)
- rand (dependencies; features: none; default-features: true; optional: false; uses: src)
- regress (dependencies; features: none; default-features: true; optional: false; uses: src)
- rustc-hash (dependencies; features: std; default-features: true; optional: false; uses: src)
- ryu-js (dependencies; features: none; default-features: true; optional: false; uses: src)
- serde (dependencies; features: derive, rc; default-features: true; optional: false; uses: src)
- serde_json (dependencies; features: none; default-features: true; optional: false; uses: src)
- small_btree (dependencies; features: none; default-features: true; optional: false; uses: src)
- static_assertions (dependencies; features: none; default-features: true; optional: false; uses: src)
- sys-locale (dependencies; features: none; default-features: true; optional: true; uses: src)
- tag_ptr (dependencies; features: none; default-features: true; optional: false; uses: src)
- tap (dependencies; features: none; default-features: true; optional: false; uses: src)
- temporal_rs (dependencies; features: none; default-features: true; optional: true; uses: src)
- thin-vec (dependencies; features: none; default-features: true; optional: false; uses: src)
- thiserror (dependencies; features: none; default-features: true; optional: false; uses: src)
- time (dependencies; features: none; default-features: true; optional: false; uses: src)
- timezone_provider (dependencies; features: none; default-features: true; optional: true; uses: src)
- tinystr (dependencies; features: none; default-features: true; optional: true; uses: src)
- writeable (dependencies; features: none; default-features: true; optional: true; uses: src)
- xsum (dependencies; features: none; default-features: true; optional: true; uses: src)
- yoke (dependencies; features: none; default-features: true; optional: true; uses: src)
- zerofrom (dependencies; features: none; default-features: true; optional: true; uses: src)

### dev-dependencies
- criterion (dev-dependencies; features: none; default-features: true; optional: false; uses: benches)
- float-cmp (dev-dependencies; features: none; default-features: true; optional: false; uses: src)
- indoc (dev-dependencies; features: none; default-features: true; optional: false; uses: src)
- test-case (dev-dependencies; features: none; default-features: true; optional: false; uses: src)
- textwrap (dev-dependencies; features: none; default-features: true; optional: false; uses: src)

### target.cfg(all(target_family = "wasm", not(any(target_os = "emscripten", target_os = "wasi")))).dependencies
- getrandom (target.cfg(all(target_family = "wasm", not(any(target_os = "emscripten", target_os = "wasi")))).dependencies; features: wasm_js; default-features: true; optional: true; uses: none)
- web-time (target.cfg(all(target_family = "wasm", not(any(target_os = "emscripten", target_os = "wasi")))).dependencies; features: none; default-features: true; optional: true; uses: none)

### target.x86_64-unknown-linux-gnu.dev-dependencies
- jemallocator (target.x86_64-unknown-linux-gnu.dev-dependencies; features: none; default-features: true; optional: false; uses: benches)

### Unused (heuristic)
arrayvec (dependencies), getrandom (target.cfg(all(target_family = "wasm", not(any(target_os = "emscripten", target_os = "wasi")))).dependencies), web-time (target.cfg(all(target_family = "wasm", not(any(target_os = "emscripten", target_os = "wasi")))).dependencies)

## boa_gc
Path: `silksurf-extras/boa/core/gc`

### dependencies
- boa_macros (dependencies; features: none; default-features: true; optional: false; uses: src)
- boa_string (dependencies; features: none; default-features: true; optional: true; uses: src)
- either (dependencies; features: none; default-features: true; optional: true; uses: src)
- hashbrown (dependencies; features: none; default-features: true; optional: false; uses: src)
- icu_locale_core (dependencies; features: none; default-features: true; optional: true; uses: src)
- thin-vec (dependencies; features: none; default-features: true; optional: true; uses: src)

## boa_icu_provider
Path: `silksurf-extras/boa/core/icu_provider`

### dependencies
- icu_casemap (dependencies; features: serde, datagen; default-features: true; optional: false; uses: none)
- icu_collator (dependencies; features: serde, datagen; default-features: true; optional: false; uses: none)
- icu_datetime (dependencies; features: serde, datagen; default-features: true; optional: false; uses: none)
- icu_decimal (dependencies; features: serde, datagen; default-features: true; optional: false; uses: none)
- icu_list (dependencies; features: serde, datagen; default-features: true; optional: false; uses: none)
- icu_locale (dependencies; features: serde, datagen; default-features: true; optional: false; uses: src)
- icu_normalizer (dependencies; features: serde, datagen; default-features: true; optional: false; uses: none)
- icu_plurals (dependencies; features: serde, datagen, experimental; default-features: true; optional: false; uses: none)
- icu_provider (dependencies; features: sync; default-features: true; optional: false; uses: src)
- icu_provider_adapters (dependencies; features: serde; default-features: true; optional: false; uses: src)
- icu_provider_blob (dependencies; features: none; default-features: true; optional: false; uses: src)
- icu_segmenter (dependencies; features: serde, datagen; default-features: true; optional: false; uses: none)
- once_cell (dependencies; features: critical-section; default-features: false; optional: false; uses: src)
- paste (dependencies; features: none; default-features: true; optional: false; uses: src)

### Unused (heuristic)
icu_casemap (dependencies), icu_collator (dependencies), icu_datetime (dependencies), icu_decimal (dependencies), icu_list (dependencies), icu_normalizer (dependencies), icu_plurals (dependencies), icu_segmenter (dependencies)

## boa_interner
Path: `silksurf-extras/boa/core/interner`

### dependencies
- arbitrary (dependencies; features: derive; default-features: true; optional: true; uses: src)
- boa_gc (dependencies; features: none; default-features: true; optional: false; uses: src)
- boa_macros (dependencies; features: none; default-features: true; optional: false; uses: src)
- hashbrown (dependencies; features: none; default-features: true; optional: false; uses: src)
- indexmap (dependencies; features: none; default-features: true; optional: false; uses: none)
- once_cell (dependencies; features: std; default-features: true; optional: false; uses: none)
- phf (dependencies; features: macros; default-features: false; optional: false; uses: none)
- rustc-hash (dependencies; features: none; default-features: false; optional: false; uses: src)
- serde (dependencies; features: derive; default-features: true; optional: true; uses: src)
- static_assertions (dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
indexmap (dependencies), once_cell (dependencies), phf (dependencies), static_assertions (dependencies)

## boa_macros
Path: `silksurf-extras/boa/core/macros`

### dependencies
- cfg-if (dependencies; features: none; default-features: true; optional: false; uses: src)
- cow-utils (dependencies; features: none; default-features: true; optional: false; uses: src)
- lz4_flex (dependencies; features: none; default-features: true; optional: true; uses: src)
- proc-macro2 (dependencies; features: none; default-features: true; optional: false; uses: src)
- quote (dependencies; features: none; default-features: true; optional: false; uses: src)
- syn (dependencies; features: full, visit-mut; default-features: true; optional: false; uses: src)
- synstructure (dependencies; features: none; default-features: true; optional: false; uses: src)

### dev-dependencies
- test-case (dev-dependencies; features: none; default-features: true; optional: false; uses: src)

## boa_parser
Path: `silksurf-extras/boa/core/parser`

### dependencies
- bitflags (dependencies; features: none; default-features: true; optional: false; uses: src)
- boa_ast (dependencies; features: none; default-features: true; optional: false; uses: src)
- boa_interner (dependencies; features: none; default-features: true; optional: false; uses: src)
- boa_macros (dependencies; features: none; default-features: true; optional: false; uses: src)
- fast-float2 (dependencies; features: none; default-features: true; optional: false; uses: src)
- icu_properties (dependencies; features: none; default-features: true; optional: false; uses: src)
- num-bigint (dependencies; features: none; default-features: true; optional: false; uses: src)
- num-traits (dependencies; features: none; default-features: true; optional: false; uses: src)
- regress (dependencies; features: none; default-features: true; optional: false; uses: src)
- rustc-hash (dependencies; features: std; default-features: true; optional: false; uses: src)

### dev-dependencies
- indoc (dev-dependencies; features: none; default-features: true; optional: false; uses: src)

## boa_runtime
Path: `silksurf-extras/boa/core/runtime`

### dependencies
- boa_engine (dependencies; features: none; default-features: true; optional: false; uses: src, tests)
- boa_gc (dependencies; features: none; default-features: true; optional: false; uses: src)
- bytemuck (dependencies; features: none; default-features: true; optional: false; uses: src)
- either (dependencies; features: none; default-features: true; optional: true; uses: src)
- futures (dependencies; features: none; default-features: true; optional: false; uses: src)
- futures-lite (dependencies; features: none; default-features: true; optional: false; uses: src)
- http (dependencies; features: none; default-features: true; optional: true; uses: src)
- reqwest (dependencies; features: none; default-features: true; optional: true; uses: src)
- rustc-hash (dependencies; features: std; default-features: true; optional: false; uses: src)
- serde_json (dependencies; features: none; default-features: true; optional: true; uses: src)
- url (dependencies; features: none; default-features: true; optional: true; uses: src)

### dev-dependencies
- indoc (dev-dependencies; features: none; default-features: true; optional: false; uses: src)
- rstest (dev-dependencies; features: none; default-features: true; optional: false; uses: tests)
- test-case (dev-dependencies; features: none; default-features: true; optional: false; uses: src)
- textwrap (dev-dependencies; features: none; default-features: true; optional: false; uses: src)

## boa_string
Path: `silksurf-extras/boa/core/string`

### dependencies
- fast-float2 (dependencies; features: none; default-features: true; optional: false; uses: src)
- itoa (dependencies; features: none; default-features: true; optional: false; uses: none)
- paste (dependencies; features: none; default-features: true; optional: false; uses: src)
- rustc-hash (dependencies; features: std; default-features: true; optional: false; uses: src)
- ryu-js (dependencies; features: none; default-features: true; optional: false; uses: none)
- static_assertions (dependencies; features: none; default-features: true; optional: false; uses: src)

### Unused (heuristic)
itoa (dependencies), ryu-js (dependencies)

## boa_examples
Path: `silksurf-extras/boa/examples`

### dependencies
- boa_ast (dependencies; features: none; default-features: true; optional: false; uses: src)
- boa_engine (dependencies; features: annex-b; default-features: true; optional: false; uses: src)
- boa_gc (dependencies; features: none; default-features: true; optional: false; uses: src)
- boa_interner (dependencies; features: none; default-features: true; optional: false; uses: src)
- boa_parser (dependencies; features: none; default-features: true; optional: false; uses: src)
- boa_runtime (dependencies; features: none; default-features: true; optional: false; uses: src)
- futures-concurrency (dependencies; features: none; default-features: true; optional: false; uses: src)
- futures-lite (dependencies; features: none; default-features: true; optional: false; uses: src)
- isahc (dependencies; features: none; default-features: true; optional: false; uses: src)
- smol (dependencies; features: none; default-features: true; optional: false; uses: src)
- time (dependencies; features: none; default-features: true; optional: false; uses: src)
- tokio (dependencies; features: rt, rt-multi-thread, time, macros; default-features: true; optional: false; uses: src)

## boa_wasm
Path: `silksurf-extras/boa/ffi/wasm`

### dependencies
- boa_engine (dependencies; features: js; default-features: true; optional: false; uses: src)
- console_error_panic_hook (dependencies; features: none; default-features: true; optional: false; uses: src)
- wasm-bindgen (dependencies; features: none; default-features: false; optional: false; uses: src)

### target.cfg(all(any(target_arch = "wasm32", target_arch = "wasm64"), target_os = "unknown")).dev-dependencies
- wasm-bindgen-test (target.cfg(all(any(target_arch = "wasm32", target_arch = "wasm64"), target_os = "unknown")).dev-dependencies; features: none; default-features: true; optional: false; uses: tests)

## boa-fuzz
Path: `silksurf-extras/boa/fuzz`

### dependencies
- boa_engine (dependencies; features: none; default-features: true; optional: false; uses: none)
- boa_interner (dependencies; features: none; default-features: true; optional: false; uses: none)
- boa_parser (dependencies; features: none; default-features: true; optional: false; uses: none)
- libfuzzer-sys (dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
boa_engine (dependencies), boa_interner (dependencies), boa_parser (dependencies), libfuzzer-sys (dependencies)

## boa_fuzz
Path: `silksurf-extras/boa/tests/fuzz`

### dependencies
- arbitrary (dependencies; features: none; default-features: true; optional: false; uses: none)
- boa_ast (dependencies; features: arbitrary; default-features: true; optional: false; uses: none)
- boa_engine (dependencies; features: fuzz; default-features: true; optional: false; uses: none)
- boa_interner (dependencies; features: arbitrary; default-features: true; optional: false; uses: none)
- boa_parser (dependencies; features: none; default-features: true; optional: false; uses: none)
- libfuzzer-sys (dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
arbitrary (dependencies), boa_ast (dependencies), boa_engine (dependencies), boa_interner (dependencies), boa_parser (dependencies), libfuzzer-sys (dependencies)

## boa_macros_tests
Path: `silksurf-extras/boa/tests/macros`

### dev-dependencies
- boa_engine (dev-dependencies; features: embedded_lz4; default-features: true; optional: false; uses: tests)
- boa_gc (dev-dependencies; features: none; default-features: true; optional: false; uses: tests)
- trybuild (dev-dependencies; features: none; default-features: true; optional: false; uses: tests)

## boa_tester
Path: `silksurf-extras/boa/tests/tester`

### dependencies
- bitflags (dependencies; features: none; default-features: true; optional: false; uses: src)
- boa_engine (dependencies; features: float16; default-features: true; optional: false; uses: src)
- boa_gc (dependencies; features: none; default-features: true; optional: false; uses: src)
- boa_runtime (dependencies; features: none; default-features: true; optional: false; uses: src)
- bus (dependencies; features: none; default-features: true; optional: false; uses: src)
- clap (dependencies; features: derive; default-features: true; optional: false; uses: src)
- color-eyre (dependencies; features: none; default-features: true; optional: false; uses: src)
- colored (dependencies; features: none; default-features: true; optional: false; uses: src)
- comfy-table (dependencies; features: none; default-features: true; optional: false; uses: src)
- cow-utils (dependencies; features: none; default-features: true; optional: false; uses: src)
- phf (dependencies; features: macros; default-features: true; optional: false; uses: src)
- rayon (dependencies; features: none; default-features: true; optional: false; uses: src)
- rustc-hash (dependencies; features: std; default-features: true; optional: false; uses: src)
- serde (dependencies; features: derive; default-features: true; optional: false; uses: src)
- serde_json (dependencies; features: none; default-features: true; optional: false; uses: src)
- serde_repr (dependencies; features: none; default-features: true; optional: false; uses: src)
- serde_yaml (dependencies; features: none; default-features: true; optional: false; uses: src)
- toml (dependencies; features: none; default-features: true; optional: false; uses: src)

## boa_wpt
Path: `silksurf-extras/boa/tests/wpt`

### build-dependencies
- git2 (build-dependencies; features: none; default-features: true; optional: false; uses: build)
- serde (build-dependencies; features: derive; default-features: true; optional: false; uses: build)
- toml (build-dependencies; features: none; default-features: true; optional: false; uses: build)

### dependencies
- boa_engine (dependencies; features: none; default-features: true; optional: false; uses: src)
- boa_gc (dependencies; features: none; default-features: true; optional: false; uses: src)
- boa_runtime (dependencies; features: all; default-features: true; optional: false; uses: src)
- rstest (dependencies; features: none; default-features: true; optional: false; uses: src)
- url (dependencies; features: none; default-features: true; optional: false; uses: src)

## gen-icu4x-data
Path: `silksurf-extras/boa/tools/gen-icu4x-data`

### dependencies
- icu_casemap (dependencies; features: datagen; default-features: true; optional: false; uses: src)
- icu_collator (dependencies; features: datagen; default-features: true; optional: false; uses: src)
- icu_datetime (dependencies; features: datagen; default-features: true; optional: false; uses: src)
- icu_decimal (dependencies; features: datagen; default-features: true; optional: false; uses: src)
- icu_list (dependencies; features: datagen; default-features: true; optional: false; uses: src)
- icu_locale (dependencies; features: datagen; default-features: true; optional: false; uses: src)
- icu_normalizer (dependencies; features: datagen; default-features: true; optional: false; uses: src)
- icu_plurals (dependencies; features: datagen, experimental; default-features: true; optional: false; uses: src)
- icu_provider_export (dependencies; features: blob_exporter, rayon; default-features: true; optional: false; uses: src)
- icu_provider_source (dependencies; features: networking, use_wasm, experimental; default-features: true; optional: false; uses: src)
- icu_segmenter (dependencies; features: datagen; default-features: true; optional: false; uses: src)
- log (dependencies; features: none; default-features: true; optional: false; uses: src)
- simple_logger (dependencies; features: none; default-features: true; optional: false; uses: src)

## scripts
Path: `silksurf-extras/boa/tools/scripts`

### dependencies
- cargo_metadata (dependencies; features: none; default-features: true; optional: false; uses: src)
- log (dependencies; features: none; default-features: true; optional: false; uses: src)
- simple_logger (dependencies; features: none; default-features: true; optional: false; uses: src)

## small_btree
Path: `silksurf-extras/boa/utils/small_btree`

### dependencies
- arrayvec (dependencies; features: none; default-features: true; optional: false; uses: src)

## servo_allocator
Path: `silksurf-extras/servo/components/allocator`

### dependencies
- backtrace (dependencies; features: none; default-features: true; optional: true; uses: none)
- log (dependencies; features: none; default-features: true; optional: true; uses: none)
- rustc-hash (dependencies; features: none; default-features: true; optional: true; uses: none)

### target.cfg(not(any(windows, target_env = "ohos"))).dependencies
- libc (target.cfg(not(any(windows, target_env = "ohos"))).dependencies; features: none; default-features: true; optional: true; uses: none)
- tikv-jemalloc-sys (target.cfg(not(any(windows, target_env = "ohos"))).dependencies; features: none; default-features: true; optional: false; uses: none)
- tikv-jemallocator (target.cfg(not(any(windows, target_env = "ohos"))).dependencies; features: none; default-features: true; optional: false; uses: none)

### target.cfg(target_env = "ohos").dependencies
- libc (target.cfg(target_env = "ohos").dependencies; features: none; default-features: true; optional: false; uses: none)

### target.cfg(windows).dependencies
- windows-sys (target.cfg(windows).dependencies; features: Win32_System_Memory; default-features: true; optional: false; uses: none)

### Unused (heuristic)
backtrace (dependencies), libc (target.cfg(not(any(windows, target_env = "ohos"))).dependencies), libc (target.cfg(target_env = "ohos").dependencies), log (dependencies), rustc-hash (dependencies), tikv-jemalloc-sys (target.cfg(not(any(windows, target_env = "ohos"))).dependencies), tikv-jemallocator (target.cfg(not(any(windows, target_env = "ohos"))).dependencies), windows-sys (target.cfg(windows).dependencies)

## background_hang_monitor
Path: `silksurf-extras/servo/components/background_hang_monitor`

### dependencies
- background_hang_monitor_api (dependencies; features: none; default-features: true; optional: false; uses: tests)
- backtrace (dependencies; features: none; default-features: true; optional: false; uses: none)
- base (dependencies; features: none; default-features: true; optional: false; uses: tests)
- crossbeam-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- libc (dependencies; features: none; default-features: true; optional: false; uses: none)
- log (dependencies; features: none; default-features: true; optional: false; uses: none)
- rustc-demangle (dependencies; features: none; default-features: true; optional: true; uses: none)
- rustc-hash (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde_json (dependencies; features: none; default-features: true; optional: false; uses: none)

### target.cfg(all(target_os = "linux", not(any(target_arch = "arm", target_arch = "aarch64", target_env = "ohos", target_env = "musl")))).dependencies
- nix (target.cfg(all(target_os = "linux", not(any(target_arch = "arm", target_arch = "aarch64", target_env = "ohos", target_env = "musl")))).dependencies; features: signal; default-features: true; optional: true; uses: none)

### target.cfg(target_os = "android").dependencies
- nix (target.cfg(target_os = "android").dependencies; features: signal; default-features: true; optional: true; uses: none)

### target.cfg(target_os = "macos").dependencies
- mach2 (target.cfg(target_os = "macos").dependencies; features: none; default-features: true; optional: true; uses: none)

### Unused (heuristic)
backtrace (dependencies), crossbeam-channel (dependencies), libc (dependencies), log (dependencies), mach2 (target.cfg(target_os = "macos").dependencies), nix (target.cfg(all(target_os = "linux", not(any(target_arch = "arm", target_arch = "aarch64", target_env = "ohos", target_env = "musl")))).dependencies), nix (target.cfg(target_os = "android").dependencies), rustc-demangle (dependencies), rustc-hash (dependencies), serde_json (dependencies)

## bluetooth
Path: `silksurf-extras/servo/components/bluetooth`

### dependencies
- base (dependencies; features: none; default-features: true; optional: false; uses: none)
- bitflags (dependencies; features: none; default-features: true; optional: false; uses: none)
- bluetooth_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- blurmock (dependencies; features: none; default-features: true; optional: true; uses: none)
- embedder_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- log (dependencies; features: none; default-features: true; optional: false; uses: none)
- rand (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_config (dependencies; features: none; default-features: true; optional: false; uses: none)
- uuid (dependencies; features: none; default-features: true; optional: false; uses: none)

### target.cfg(target_os = "android").dependencies
- blurdroid (target.cfg(target_os = "android").dependencies; features: none; default-features: true; optional: true; uses: none)

### target.cfg(target_os = "linux").dependencies
- blurz (target.cfg(target_os = "linux").dependencies; features: none; default-features: true; optional: true; uses: none)

### target.cfg(target_os = "macos").dependencies
- blurmac (target.cfg(target_os = "macos").dependencies; features: none; default-features: true; optional: true; uses: none)

### Unused (heuristic)
base (dependencies), bitflags (dependencies), bluetooth_traits (dependencies), blurdroid (target.cfg(target_os = "android").dependencies), blurmac (target.cfg(target_os = "macos").dependencies), blurmock (dependencies), blurz (target.cfg(target_os = "linux").dependencies), embedder_traits (dependencies), log (dependencies), rand (dependencies), servo_config (dependencies), uuid (dependencies)

## canvas
Path: `silksurf-extras/servo/components/canvas`

### dependencies
- app_units (dependencies; features: none; default-features: true; optional: false; uses: none)
- base (dependencies; features: none; default-features: true; optional: false; uses: none)
- bytemuck (dependencies; features: extern_crate_alloc; default-features: true; optional: false; uses: none)
- canvas_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- compositing_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- crossbeam-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- cssparser (dependencies; features: none; default-features: true; optional: false; uses: none)
- euclid (dependencies; features: none; default-features: true; optional: false; uses: none)
- fonts (dependencies; features: none; default-features: true; optional: false; uses: none)
- futures-intrusive (dependencies; features: none; default-features: true; optional: true; uses: none)
- ipc-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- kurbo (dependencies; features: none; default-features: true; optional: false; uses: none)
- log (dependencies; features: none; default-features: true; optional: false; uses: none)
- net_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- peniko (dependencies; features: none; default-features: true; optional: true; uses: none)
- pixels (dependencies; features: none; default-features: true; optional: false; uses: none)
- pollster (dependencies; features: none; default-features: true; optional: true; uses: none)
- profile_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- range (dependencies; features: none; default-features: true; optional: false; uses: none)
- rustc-hash (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo-tracing (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_arc (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_config (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo (dependencies; features: none; default-features: true; optional: false; uses: none)
- tracing (dependencies; features: none; default-features: true; optional: true; uses: none)
- unicode-script (dependencies; features: none; default-features: true; optional: false; uses: none)
- vello (dependencies; features: none; default-features: true; optional: true; uses: none)
- vello_cpu (dependencies; features: none; default-features: true; optional: true; uses: none)
- webrender_api (dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
app_units (dependencies), base (dependencies), bytemuck (dependencies), canvas_traits (dependencies), compositing_traits (dependencies), crossbeam-channel (dependencies), cssparser (dependencies), euclid (dependencies), fonts (dependencies), futures-intrusive (dependencies), ipc-channel (dependencies), kurbo (dependencies), log (dependencies), net_traits (dependencies), peniko (dependencies), pixels (dependencies), pollster (dependencies), profile_traits (dependencies), range (dependencies), rustc-hash (dependencies), servo-tracing (dependencies), servo_arc (dependencies), servo_config (dependencies), stylo (dependencies), tracing (dependencies), unicode-script (dependencies), vello (dependencies), vello_cpu (dependencies), webrender_api (dependencies)

## compositing
Path: `silksurf-extras/servo/components/compositing`

### dependencies
- base (dependencies; features: none; default-features: true; optional: false; uses: none)
- bincode (dependencies; features: none; default-features: true; optional: false; uses: none)
- bitflags (dependencies; features: none; default-features: true; optional: false; uses: none)
- canvas_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- compositing_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- constellation_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- crossbeam-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- dpi (dependencies; features: none; default-features: true; optional: false; uses: none)
- embedder_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- euclid (dependencies; features: none; default-features: true; optional: false; uses: none)
- gleam (dependencies; features: none; default-features: true; optional: false; uses: none)
- image (dependencies; features: none; default-features: true; optional: false; uses: none)
- ipc-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- libc (dependencies; features: none; default-features: true; optional: false; uses: none)
- log (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of (dependencies; features: none; default-features: true; optional: false; uses: none)
- media (dependencies; features: none; default-features: true; optional: false; uses: none)
- parking_lot (dependencies; features: none; default-features: true; optional: false; uses: none)
- pixels (dependencies; features: none; default-features: true; optional: false; uses: none)
- profile_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- rayon (dependencies; features: none; default-features: true; optional: false; uses: none)
- rustc-hash (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo-tracing (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_allocator (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_config (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_geometry (dependencies; features: none; default-features: true; optional: false; uses: none)
- smallvec (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- surfman (dependencies; features: none; default-features: true; optional: false; uses: none)
- timers (dependencies; features: none; default-features: true; optional: false; uses: none)
- tracing (dependencies; features: none; default-features: true; optional: true; uses: none)
- webgl (dependencies; features: none; default-features: true; optional: false; uses: none)
- webgpu (dependencies; features: none; default-features: true; optional: true; uses: none)
- webrender (dependencies; features: none; default-features: true; optional: false; uses: none)
- webrender_api (dependencies; features: none; default-features: true; optional: false; uses: none)
- webxr (dependencies; features: none; default-features: true; optional: true; uses: none)
- webxr-api (dependencies; features: none; default-features: true; optional: true; uses: none)
- wr_malloc_size_of (dependencies; features: none; default-features: true; optional: false; uses: none)

### dev-dependencies
- surfman (dev-dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
base (dependencies), bincode (dependencies), bitflags (dependencies), canvas_traits (dependencies), compositing_traits (dependencies), constellation_traits (dependencies), crossbeam-channel (dependencies), dpi (dependencies), embedder_traits (dependencies), euclid (dependencies), gleam (dependencies), image (dependencies), ipc-channel (dependencies), libc (dependencies), log (dependencies), malloc_size_of (dependencies), media (dependencies), parking_lot (dependencies), pixels (dependencies), profile_traits (dependencies), rayon (dependencies), rustc-hash (dependencies), servo-tracing (dependencies), servo_allocator (dependencies), servo_config (dependencies), servo_geometry (dependencies), smallvec (dependencies), stylo_traits (dependencies), surfman (dependencies), surfman (dev-dependencies), timers (dependencies), tracing (dependencies), webgl (dependencies), webgpu (dependencies), webrender (dependencies), webrender_api (dependencies), webxr (dependencies), webxr-api (dependencies), wr_malloc_size_of (dependencies)

## servo_config
Path: `silksurf-extras/servo/components/config`

### dependencies
- serde (dependencies; features: derive; default-features: true; optional: false; uses: none)
- serde_json (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_config_macro (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_url (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo_config (dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
serde (dependencies), serde_json (dependencies), servo_config_macro (dependencies), servo_url (dependencies), stylo_config (dependencies)

## servo_config_macro
Path: `silksurf-extras/servo/components/config/macro`

### dependencies
- proc-macro2 (dependencies; features: none; default-features: true; optional: false; uses: none)
- quote (dependencies; features: none; default-features: true; optional: false; uses: none)
- syn (dependencies; features: none; default-features: true; optional: false; uses: none)
- synstructure (dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
proc-macro2 (dependencies), quote (dependencies), syn (dependencies), synstructure (dependencies)

## constellation
Path: `silksurf-extras/servo/components/constellation`

### dependencies
- background_hang_monitor (dependencies; features: none; default-features: true; optional: false; uses: none)
- background_hang_monitor_api (dependencies; features: none; default-features: true; optional: false; uses: none)
- backtrace (dependencies; features: none; default-features: true; optional: false; uses: none)
- base (dependencies; features: none; default-features: true; optional: false; uses: none)
- bluetooth_traits (dependencies; features: none; default-features: true; optional: true; uses: none)
- canvas (dependencies; features: none; default-features: true; optional: false; uses: none)
- canvas_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- compositing_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- constellation_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- content-security-policy (dependencies; features: none; default-features: true; optional: false; uses: none)
- crossbeam-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- devtools_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- embedder_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- euclid (dependencies; features: none; default-features: true; optional: false; uses: none)
- fonts (dependencies; features: none; default-features: true; optional: false; uses: none)
- ipc-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- keyboard-types (dependencies; features: none; default-features: true; optional: false; uses: none)
- layout_api (dependencies; features: none; default-features: true; optional: false; uses: none)
- log (dependencies; features: none; default-features: true; optional: false; uses: none)
- media (dependencies; features: none; default-features: true; optional: false; uses: none)
- net (dependencies; features: none; default-features: true; optional: false; uses: none)
- net_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- parking_lot (dependencies; features: none; default-features: true; optional: false; uses: none)
- profile (dependencies; features: none; default-features: true; optional: false; uses: none)
- profile_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- rand (dependencies; features: none; default-features: true; optional: false; uses: none)
- rustc-hash (dependencies; features: none; default-features: true; optional: false; uses: none)
- script_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo-tracing (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_config (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_url (dependencies; features: none; default-features: true; optional: false; uses: none)
- storage_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- tracing (dependencies; features: none; default-features: true; optional: true; uses: none)
- webgpu (dependencies; features: none; default-features: true; optional: false; uses: none)
- webgpu_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- webrender (dependencies; features: none; default-features: true; optional: false; uses: none)
- webrender_api (dependencies; features: none; default-features: true; optional: false; uses: none)
- webxr-api (dependencies; features: ipc; default-features: true; optional: false; uses: none)

### target.cfg(any(target_os="macos", all(not(target_os = "windows"), not(target_os = "ios"), not(target_os="android"), not(target_env="ohos"), not(target_arch="arm"), not(target_arch="aarch64")))).dependencies
- gaol (target.cfg(any(target_os="macos", all(not(target_os = "windows"), not(target_os = "ios"), not(target_os="android"), not(target_env="ohos"), not(target_arch="arm"), not(target_arch="aarch64")))).dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
background_hang_monitor (dependencies), background_hang_monitor_api (dependencies), backtrace (dependencies), base (dependencies), bluetooth_traits (dependencies), canvas (dependencies), canvas_traits (dependencies), compositing_traits (dependencies), constellation_traits (dependencies), content-security-policy (dependencies), crossbeam-channel (dependencies), devtools_traits (dependencies), embedder_traits (dependencies), euclid (dependencies), fonts (dependencies), gaol (target.cfg(any(target_os="macos", all(not(target_os = "windows"), not(target_os = "ios"), not(target_os="android"), not(target_env="ohos"), not(target_arch="arm"), not(target_arch="aarch64")))).dependencies), ipc-channel (dependencies), keyboard-types (dependencies), layout_api (dependencies), log (dependencies), media (dependencies), net (dependencies), net_traits (dependencies), parking_lot (dependencies), profile (dependencies), profile_traits (dependencies), rand (dependencies), rustc-hash (dependencies), script_traits (dependencies), serde (dependencies), servo-tracing (dependencies), servo_config (dependencies), servo_url (dependencies), storage_traits (dependencies), stylo (dependencies), stylo_traits (dependencies), tracing (dependencies), webgpu (dependencies), webgpu_traits (dependencies), webrender (dependencies), webrender_api (dependencies), webxr-api (dependencies)

## deny_public_fields
Path: `silksurf-extras/servo/components/deny_public_fields`

### dependencies
- syn (dependencies; features: none; default-features: true; optional: false; uses: none)
- synstructure (dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
syn (dependencies), synstructure (dependencies)

## devtools
Path: `silksurf-extras/servo/components/devtools`

### build-dependencies
- chrono (build-dependencies; features: none; default-features: true; optional: false; uses: build)

### dependencies
- base (dependencies; features: none; default-features: true; optional: false; uses: none)
- base64 (dependencies; features: none; default-features: true; optional: false; uses: none)
- chrono (dependencies; features: none; default-features: true; optional: false; uses: build)
- crossbeam-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- devtools_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- embedder_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- headers (dependencies; features: none; default-features: true; optional: false; uses: none)
- http (dependencies; features: none; default-features: true; optional: false; uses: none)
- log (dependencies; features: none; default-features: true; optional: false; uses: none)
- net (dependencies; features: none; default-features: true; optional: false; uses: none)
- net_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- rand (dependencies; features: none; default-features: true; optional: false; uses: none)
- rustc-hash (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde_json (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_config (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_url (dependencies; features: none; default-features: true; optional: false; uses: none)
- uuid (dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
base (dependencies), base64 (dependencies), crossbeam-channel (dependencies), devtools_traits (dependencies), embedder_traits (dependencies), headers (dependencies), http (dependencies), log (dependencies), net (dependencies), net_traits (dependencies), rand (dependencies), rustc-hash (dependencies), serde (dependencies), serde_json (dependencies), servo_config (dependencies), servo_url (dependencies), uuid (dependencies)

## dom_struct
Path: `silksurf-extras/servo/components/dom_struct`

### dependencies
- quote (dependencies; features: none; default-features: true; optional: false; uses: none)
- syn (dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
quote (dependencies), syn (dependencies)

## domobject_derive
Path: `silksurf-extras/servo/components/domobject_derive`

### dependencies
- proc-macro2 (dependencies; features: none; default-features: true; optional: false; uses: none)
- quote (dependencies; features: none; default-features: true; optional: false; uses: none)
- syn (dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
proc-macro2 (dependencies), quote (dependencies), syn (dependencies)

## fonts
Path: `silksurf-extras/servo/components/fonts`

### dependencies
- app_units (dependencies; features: none; default-features: true; optional: false; uses: tests)
- base (dependencies; features: none; default-features: true; optional: false; uses: none)
- bitflags (dependencies; features: none; default-features: true; optional: false; uses: none)
- compositing_traits (dependencies; features: none; default-features: true; optional: false; uses: tests)
- content-security-policy (dependencies; features: none; default-features: true; optional: false; uses: none)
- euclid (dependencies; features: none; default-features: true; optional: false; uses: tests)
- fonts_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- fontsan (dependencies; features: none; default-features: true; optional: false; uses: none)
- harfbuzz-sys (dependencies; features: bundled; default-features: true; optional: false; uses: none)
- ipc-channel (dependencies; features: none; default-features: true; optional: false; uses: tests)
- itertools (dependencies; features: none; default-features: true; optional: false; uses: none)
- libc (dependencies; features: none; default-features: true; optional: false; uses: none)
- log (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of_derive (dependencies; features: none; default-features: true; optional: false; uses: none)
- memmap2 (dependencies; features: none; default-features: true; optional: false; uses: none)
- net_traits (dependencies; features: none; default-features: true; optional: false; uses: tests)
- num-traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- parking_lot (dependencies; features: none; default-features: true; optional: false; uses: tests)
- profile_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- range (dependencies; features: none; default-features: true; optional: false; uses: none)
- read-fonts (dependencies; features: none; default-features: true; optional: false; uses: none)
- rustc-hash (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo-tracing (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_arc (dependencies; features: none; default-features: true; optional: false; uses: tests)
- servo_config (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_url (dependencies; features: none; default-features: true; optional: false; uses: tests)
- skrifa (dependencies; features: none; default-features: true; optional: false; uses: none)
- smallvec (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo_atoms (dependencies; features: none; default-features: true; optional: false; uses: tests)
- tracing (dependencies; features: none; default-features: true; optional: true; uses: none)
- unicode-properties (dependencies; features: none; default-features: true; optional: false; uses: none)
- unicode-script (dependencies; features: none; default-features: true; optional: false; uses: tests)
- url (dependencies; features: none; default-features: true; optional: false; uses: none)
- webrender_api (dependencies; features: none; default-features: true; optional: false; uses: tests)

### target.cfg(all(target_os = "linux", not(target_env = "ohos"))).dependencies
- fontconfig_sys (target.cfg(all(target_os = "linux", not(target_env = "ohos"))).dependencies; features: none; default-features: true; optional: false; uses: none)

### target.cfg(any(target_os = "linux", target_os = "android")).dependencies
- freetype-sys (target.cfg(any(target_os = "linux", target_os = "android")).dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_allocator (target.cfg(any(target_os = "linux", target_os = "android")).dependencies; features: none; default-features: true; optional: false; uses: none)

### target.cfg(target_os = "android").dependencies
- xml (target.cfg(target_os = "android").dependencies; features: none; default-features: true; optional: false; uses: none)

### target.cfg(target_os = "macos").dependencies
- byteorder (target.cfg(target_os = "macos").dependencies; features: none; default-features: true; optional: false; uses: none)
- core-foundation (target.cfg(target_os = "macos").dependencies; features: none; default-features: true; optional: false; uses: none)
- core-graphics (target.cfg(target_os = "macos").dependencies; features: none; default-features: true; optional: false; uses: none)
- core-text (target.cfg(target_os = "macos").dependencies; features: none; default-features: true; optional: false; uses: none)

### target.cfg(target_os = "windows").dependencies
- dwrote (target.cfg(target_os = "windows").dependencies; features: none; default-features: true; optional: false; uses: none)
- winapi (target.cfg(target_os = "windows").dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
base (dependencies), bitflags (dependencies), byteorder (target.cfg(target_os = "macos").dependencies), content-security-policy (dependencies), core-foundation (target.cfg(target_os = "macos").dependencies), core-graphics (target.cfg(target_os = "macos").dependencies), core-text (target.cfg(target_os = "macos").dependencies), dwrote (target.cfg(target_os = "windows").dependencies), fontconfig_sys (target.cfg(all(target_os = "linux", not(target_env = "ohos"))).dependencies), fonts_traits (dependencies), fontsan (dependencies), freetype-sys (target.cfg(any(target_os = "linux", target_os = "android")).dependencies), harfbuzz-sys (dependencies), itertools (dependencies), libc (dependencies), log (dependencies), malloc_size_of (dependencies), malloc_size_of_derive (dependencies), memmap2 (dependencies), num-traits (dependencies), profile_traits (dependencies), range (dependencies), read-fonts (dependencies), rustc-hash (dependencies), serde (dependencies), servo-tracing (dependencies), servo_allocator (target.cfg(any(target_os = "linux", target_os = "android")).dependencies), servo_config (dependencies), skrifa (dependencies), smallvec (dependencies), stylo (dependencies), tracing (dependencies), unicode-properties (dependencies), url (dependencies), winapi (target.cfg(target_os = "windows").dependencies), xml (target.cfg(target_os = "android").dependencies)

## servo_geometry
Path: `silksurf-extras/servo/components/geometry`

### dependencies
- app_units (dependencies; features: none; default-features: true; optional: false; uses: none)
- euclid (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of_derive (dependencies; features: none; default-features: true; optional: false; uses: none)
- webrender (dependencies; features: none; default-features: true; optional: false; uses: none)
- webrender_api (dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
app_units (dependencies), euclid (dependencies), malloc_size_of (dependencies), malloc_size_of_derive (dependencies), webrender (dependencies), webrender_api (dependencies)

## hyper_serde
Path: `silksurf-extras/servo/components/hyper_serde`

### dependencies
- cookie (dependencies; features: none; default-features: true; optional: false; uses: tests)
- headers (dependencies; features: none; default-features: true; optional: false; uses: tests)
- http (dependencies; features: none; default-features: true; optional: false; uses: tests)
- hyper (dependencies; features: none; default-features: true; optional: false; uses: tests)
- mime (dependencies; features: none; default-features: true; optional: false; uses: tests)
- serde_bytes (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde_core (dependencies; features: none; default-features: true; optional: false; uses: none)

### dev-dependencies
- serde (dev-dependencies; features: none; default-features: true; optional: false; uses: tests)
- serde_test (dev-dependencies; features: none; default-features: true; optional: false; uses: tests)

### Unused (heuristic)
serde_bytes (dependencies), serde_core (dependencies)

## jstraceable_derive
Path: `silksurf-extras/servo/components/jstraceable_derive`

### dependencies
- proc-macro2 (dependencies; features: none; default-features: true; optional: false; uses: none)
- syn (dependencies; features: none; default-features: true; optional: false; uses: none)
- synstructure (dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
proc-macro2 (dependencies), syn (dependencies), synstructure (dependencies)

## layout
Path: `silksurf-extras/servo/components/layout`

### dependencies
- app_units (dependencies; features: none; default-features: true; optional: false; uses: tests)
- atomic_refcell (dependencies; features: none; default-features: true; optional: false; uses: none)
- base (dependencies; features: none; default-features: true; optional: false; uses: none)
- bitflags (dependencies; features: none; default-features: true; optional: false; uses: none)
- compositing_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- cssparser (dependencies; features: none; default-features: true; optional: false; uses: none)
- data-url (dependencies; features: none; default-features: true; optional: false; uses: none)
- embedder_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- euclid (dependencies; features: none; default-features: true; optional: false; uses: tests)
- fonts (dependencies; features: none; default-features: true; optional: false; uses: none)
- fonts_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- html5ever (dependencies; features: none; default-features: true; optional: false; uses: none)
- icu_locid (dependencies; features: none; default-features: true; optional: false; uses: none)
- icu_segmenter (dependencies; features: none; default-features: true; optional: false; uses: none)
- itertools (dependencies; features: none; default-features: true; optional: false; uses: none)
- kurbo (dependencies; features: none; default-features: true; optional: false; uses: none)
- layout_api (dependencies; features: none; default-features: true; optional: false; uses: none)
- log (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of_derive (dependencies; features: none; default-features: true; optional: false; uses: none)
- net_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- parking_lot (dependencies; features: none; default-features: true; optional: false; uses: none)
- pixels (dependencies; features: none; default-features: true; optional: false; uses: none)
- profile_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- range (dependencies; features: none; default-features: true; optional: false; uses: none)
- rayon (dependencies; features: none; default-features: true; optional: false; uses: none)
- rustc-hash (dependencies; features: none; default-features: true; optional: false; uses: none)
- script (dependencies; features: none; default-features: true; optional: false; uses: none)
- script_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- selectors (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo-tracing (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_arc (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_config (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_geometry (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_url (dependencies; features: none; default-features: true; optional: false; uses: none)
- smallvec (dependencies; features: none; default-features: true; optional: false; uses: none)
- strum (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo_atoms (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- taffy (dependencies; features: none; default-features: true; optional: false; uses: none)
- tracing (dependencies; features: none; default-features: true; optional: true; uses: none)
- unicode-bidi (dependencies; features: none; default-features: true; optional: false; uses: none)
- unicode-script (dependencies; features: none; default-features: true; optional: false; uses: none)
- url (dependencies; features: none; default-features: true; optional: false; uses: none)
- webrender_api (dependencies; features: none; default-features: true; optional: false; uses: none)
- xi-unicode (dependencies; features: none; default-features: true; optional: false; uses: none)

### dev-dependencies
- num-traits (dev-dependencies; features: none; default-features: true; optional: false; uses: tests)
- quickcheck (dev-dependencies; features: none; default-features: true; optional: false; uses: tests)

### Unused (heuristic)
atomic_refcell (dependencies), base (dependencies), bitflags (dependencies), compositing_traits (dependencies), cssparser (dependencies), data-url (dependencies), embedder_traits (dependencies), fonts (dependencies), fonts_traits (dependencies), html5ever (dependencies), icu_locid (dependencies), icu_segmenter (dependencies), itertools (dependencies), kurbo (dependencies), layout_api (dependencies), log (dependencies), malloc_size_of (dependencies), malloc_size_of_derive (dependencies), net_traits (dependencies), parking_lot (dependencies), pixels (dependencies), profile_traits (dependencies), range (dependencies), rayon (dependencies), rustc-hash (dependencies), script (dependencies), script_traits (dependencies), selectors (dependencies), servo-tracing (dependencies), servo_arc (dependencies), servo_config (dependencies), servo_geometry (dependencies), servo_url (dependencies), smallvec (dependencies), strum (dependencies), stylo (dependencies), stylo_atoms (dependencies), stylo_traits (dependencies), taffy (dependencies), tracing (dependencies), unicode-bidi (dependencies), unicode-script (dependencies), url (dependencies), webrender_api (dependencies), xi-unicode (dependencies)

## servo_malloc_size_of
Path: `silksurf-extras/servo/components/malloc_size_of`

### dependencies
- accountable-refcell (dependencies; features: none; default-features: true; optional: false; uses: none)
- app_units (dependencies; features: none; default-features: true; optional: false; uses: none)
- atomic_refcell (dependencies; features: none; default-features: true; optional: false; uses: none)
- content-security-policy (dependencies; features: none; default-features: true; optional: false; uses: none)
- crossbeam-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- euclid (dependencies; features: none; default-features: true; optional: false; uses: none)
- http (dependencies; features: none; default-features: true; optional: false; uses: none)
- indexmap (dependencies; features: none; default-features: true; optional: false; uses: none)
- ipc-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- keyboard-types (dependencies; features: none; default-features: true; optional: false; uses: none)
- markup5ever (dependencies; features: none; default-features: true; optional: false; uses: none)
- mime (dependencies; features: none; default-features: true; optional: false; uses: none)
- parking_lot (dependencies; features: none; default-features: true; optional: false; uses: none)
- resvg (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_allocator (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_arc (dependencies; features: none; default-features: true; optional: false; uses: none)
- smallvec (dependencies; features: none; default-features: true; optional: false; uses: none)
- string_cache (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo_dom (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo_malloc_size_of (dependencies; features: none; default-features: true; optional: false; uses: none)
- taffy (dependencies; features: none; default-features: true; optional: false; uses: none)
- tendril (dependencies; features: none; default-features: true; optional: false; uses: none)
- tokio (dependencies; features: sync; default-features: true; optional: false; uses: none)
- unicode-bidi (dependencies; features: none; default-features: true; optional: false; uses: none)
- unicode-script (dependencies; features: none; default-features: true; optional: false; uses: none)
- url (dependencies; features: none; default-features: true; optional: false; uses: none)
- urlpattern (dependencies; features: none; default-features: true; optional: false; uses: none)
- utf-8 (dependencies; features: none; default-features: true; optional: false; uses: none)
- uuid (dependencies; features: none; default-features: true; optional: false; uses: none)
- webrender (dependencies; features: none; default-features: true; optional: false; uses: none)
- webrender_api (dependencies; features: none; default-features: true; optional: false; uses: none)
- wr_malloc_size_of (dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
accountable-refcell (dependencies), app_units (dependencies), atomic_refcell (dependencies), content-security-policy (dependencies), crossbeam-channel (dependencies), euclid (dependencies), http (dependencies), indexmap (dependencies), ipc-channel (dependencies), keyboard-types (dependencies), markup5ever (dependencies), mime (dependencies), parking_lot (dependencies), resvg (dependencies), servo_allocator (dependencies), servo_arc (dependencies), smallvec (dependencies), string_cache (dependencies), stylo (dependencies), stylo_dom (dependencies), stylo_malloc_size_of (dependencies), taffy (dependencies), tendril (dependencies), tokio (dependencies), unicode-bidi (dependencies), unicode-script (dependencies), url (dependencies), urlpattern (dependencies), utf-8 (dependencies), uuid (dependencies), webrender (dependencies), webrender_api (dependencies), wr_malloc_size_of (dependencies)

## media
Path: `silksurf-extras/servo/components/media`

### dependencies
- compositing_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- euclid (dependencies; features: none; default-features: true; optional: false; uses: none)
- ipc-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- log (dependencies; features: none; default-features: true; optional: false; uses: none)
- rustc-hash (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo-media (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_config (dependencies; features: none; default-features: true; optional: false; uses: none)
- webrender_api (dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
compositing_traits (dependencies), euclid (dependencies), ipc-channel (dependencies), log (dependencies), rustc-hash (dependencies), serde (dependencies), servo-media (dependencies), servo_config (dependencies), webrender_api (dependencies)

## metrics
Path: `silksurf-extras/servo/components/metrics`

### dependencies
- base (dependencies; features: none; default-features: true; optional: false; uses: none)
- compositing_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- log (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of_derive (dependencies; features: none; default-features: true; optional: false; uses: none)
- profile_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- script_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_config (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_url (dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
base (dependencies), compositing_traits (dependencies), log (dependencies), malloc_size_of (dependencies), malloc_size_of_derive (dependencies), profile_traits (dependencies), script_traits (dependencies), servo_config (dependencies), servo_url (dependencies)

## net
Path: `silksurf-extras/servo/components/net`

### dependencies
- async-compression (dependencies; features: brotli, gzip, tokio, zlib, zstd; default-features: false; optional: false; uses: none)
- async-recursion (dependencies; features: none; default-features: true; optional: false; uses: none)
- async-tungstenite (dependencies; features: none; default-features: true; optional: false; uses: none)
- base (dependencies; features: none; default-features: true; optional: false; uses: tests)
- base64 (dependencies; features: none; default-features: true; optional: false; uses: tests)
- bytes (dependencies; features: none; default-features: true; optional: false; uses: none)
- chrono (dependencies; features: none; default-features: true; optional: false; uses: none)
- compositing_traits (dependencies; features: none; default-features: true; optional: false; uses: tests)
- content-security-policy (dependencies; features: none; default-features: true; optional: false; uses: none)
- cookie (dependencies; features: none; default-features: true; optional: false; uses: tests)
- crossbeam-channel (dependencies; features: none; default-features: true; optional: false; uses: tests)
- data-url (dependencies; features: none; default-features: true; optional: false; uses: none)
- devtools_traits (dependencies; features: none; default-features: true; optional: false; uses: tests)
- embedder_traits (dependencies; features: none; default-features: true; optional: false; uses: tests)
- fst (dependencies; features: none; default-features: true; optional: false; uses: none)
- futures (dependencies; features: none; default-features: true; optional: false; uses: none)
- futures-core (dependencies; features: none; default-features: false; optional: false; uses: none)
- futures-util (dependencies; features: none; default-features: false; optional: false; uses: none)
- generic-array (dependencies; features: none; default-features: true; optional: false; uses: none)
- headers (dependencies; features: none; default-features: true; optional: false; uses: tests)
- http (dependencies; features: none; default-features: true; optional: false; uses: tests)
- http-body-util (dependencies; features: none; default-features: true; optional: false; uses: tests)
- hyper (dependencies; features: client, http1, http2; default-features: true; optional: false; uses: tests)
- hyper-rustls (dependencies; features: none; default-features: true; optional: false; uses: none)
- hyper-util (dependencies; features: none; default-features: true; optional: false; uses: none)
- hyper_serde (dependencies; features: none; default-features: true; optional: false; uses: tests)
- imsz (dependencies; features: none; default-features: true; optional: false; uses: none)
- ipc-channel (dependencies; features: none; default-features: true; optional: false; uses: tests)
- itertools (dependencies; features: none; default-features: true; optional: false; uses: none)
- log (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of_derive (dependencies; features: none; default-features: true; optional: false; uses: none)
- mime (dependencies; features: none; default-features: true; optional: false; uses: tests)
- mime_guess (dependencies; features: none; default-features: true; optional: false; uses: none)
- net_traits (dependencies; features: none; default-features: true; optional: false; uses: tests)
- nom (dependencies; features: none; default-features: true; optional: false; uses: none)
- parking_lot (dependencies; features: none; default-features: true; optional: false; uses: tests)
- pixels (dependencies; features: none; default-features: true; optional: false; uses: none)
- profile_traits (dependencies; features: none; default-features: true; optional: false; uses: tests)
- quick_cache (dependencies; features: ahash; default-features: false; optional: false; uses: none)
- rayon (dependencies; features: none; default-features: true; optional: false; uses: none)
- resvg (dependencies; features: none; default-features: true; optional: false; uses: none)
- rustc-hash (dependencies; features: none; default-features: true; optional: false; uses: tests)
- rustls (dependencies; features: none; default-features: true; optional: false; uses: tests)
- rustls-pki-types (dependencies; features: none; default-features: true; optional: false; uses: none)
- rustls-platform-verifier (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde_json (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_arc (dependencies; features: none; default-features: true; optional: false; uses: tests)
- servo_config (dependencies; features: none; default-features: true; optional: false; uses: tests)
- servo_url (dependencies; features: none; default-features: true; optional: false; uses: tests)
- sha2 (dependencies; features: none; default-features: true; optional: false; uses: none)
- time (dependencies; features: none; default-features: true; optional: false; uses: tests)
- tokio (dependencies; features: macros, rt-multi-thread, sync; default-features: true; optional: false; uses: tests)
- tokio-rustls (dependencies; features: none; default-features: true; optional: false; uses: none)
- tokio-stream (dependencies; features: none; default-features: true; optional: false; uses: none)
- tokio-util (dependencies; features: codec, io; default-features: false; optional: false; uses: none)
- tower (dependencies; features: none; default-features: true; optional: false; uses: none)
- tungstenite (dependencies; features: none; default-features: true; optional: false; uses: none)
- url (dependencies; features: none; default-features: true; optional: false; uses: tests)
- uuid (dependencies; features: none; default-features: true; optional: false; uses: tests)
- webpki-roots (dependencies; features: none; default-features: true; optional: false; uses: none)
- webrender_api (dependencies; features: none; default-features: true; optional: false; uses: tests)

### dev-dependencies
- embedder_traits (dev-dependencies; features: baked-default-resources; default-features: true; optional: false; uses: tests)
- flate2 (dev-dependencies; features: none; default-features: true; optional: false; uses: tests)
- fst (dev-dependencies; features: none; default-features: true; optional: false; uses: none)
- futures (dev-dependencies; features: compat; default-features: true; optional: false; uses: none)
- hyper (dev-dependencies; features: full; default-features: true; optional: false; uses: tests)
- hyper-util (dev-dependencies; features: server-graceful; default-features: true; optional: false; uses: none)
- net (dev-dependencies; features: test-util; default-features: true; optional: false; uses: tests)
- rustls (dev-dependencies; features: aws-lc-rs; default-features: true; optional: false; uses: tests)

### Unused (heuristic)
async-compression (dependencies), async-recursion (dependencies), async-tungstenite (dependencies), bytes (dependencies), chrono (dependencies), content-security-policy (dependencies), data-url (dependencies), fst (dependencies), fst (dev-dependencies), futures (dependencies), futures (dev-dependencies), futures-core (dependencies), futures-util (dependencies), generic-array (dependencies), hyper-rustls (dependencies), hyper-util (dependencies), hyper-util (dev-dependencies), imsz (dependencies), itertools (dependencies), log (dependencies), malloc_size_of (dependencies), malloc_size_of_derive (dependencies), mime_guess (dependencies), nom (dependencies), pixels (dependencies), quick_cache (dependencies), rayon (dependencies), resvg (dependencies), rustls-pki-types (dependencies), rustls-platform-verifier (dependencies), serde (dependencies), serde_json (dependencies), sha2 (dependencies), tokio-rustls (dependencies), tokio-stream (dependencies), tokio-util (dependencies), tower (dependencies), tungstenite (dependencies), webpki-roots (dependencies)

## pixels
Path: `silksurf-extras/servo/components/pixels`

### dependencies
- euclid (dependencies; features: none; default-features: true; optional: false; uses: tests)
- image (dependencies; features: none; default-features: true; optional: false; uses: none)
- ipc-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- log (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of_derive (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde (dependencies; features: derive; default-features: true; optional: false; uses: none)
- webrender_api (dependencies; features: none; default-features: true; optional: false; uses: none)

### dev-dependencies
- criterion (dev-dependencies; features: html_reports; default-features: true; optional: false; uses: none)

### Unused (heuristic)
criterion (dev-dependencies), image (dependencies), ipc-channel (dependencies), log (dependencies), malloc_size_of (dependencies), malloc_size_of_derive (dependencies), serde (dependencies), webrender_api (dependencies)

## profile
Path: `silksurf-extras/servo/components/profile`

### dependencies
- base (dependencies; features: none; default-features: true; optional: false; uses: none)
- log (dependencies; features: none; default-features: true; optional: false; uses: none)
- profile_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde_json (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_allocator (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_config (dependencies; features: none; default-features: true; optional: false; uses: none)
- time (dependencies; features: none; default-features: true; optional: false; uses: none)

### target.cfg(not(any(target_os = "windows", target_env = "ohos"))).dependencies
- tikv-jemalloc-sys (target.cfg(not(any(target_os = "windows", target_env = "ohos"))).dependencies; features: none; default-features: true; optional: false; uses: none)

### target.cfg(not(target_os = "windows")).dependencies
- libc (target.cfg(not(target_os = "windows")).dependencies; features: none; default-features: true; optional: false; uses: none)

### target.cfg(target_os = "linux").dependencies
- regex (target.cfg(target_os = "linux").dependencies; features: none; default-features: true; optional: false; uses: none)

### target.cfg(target_os = "macos").dependencies
- mach2 (target.cfg(target_os = "macos").dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
base (dependencies), libc (target.cfg(not(target_os = "windows")).dependencies), log (dependencies), mach2 (target.cfg(target_os = "macos").dependencies), profile_traits (dependencies), regex (target.cfg(target_os = "linux").dependencies), serde (dependencies), serde_json (dependencies), servo_allocator (dependencies), servo_config (dependencies), tikv-jemalloc-sys (target.cfg(not(any(target_os = "windows", target_env = "ohos"))).dependencies), time (dependencies)

## range
Path: `silksurf-extras/servo/components/range`

### dependencies
- malloc_size_of (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of_derive (dependencies; features: none; default-features: true; optional: false; uses: none)
- num-traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde (dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
malloc_size_of (dependencies), malloc_size_of_derive (dependencies), num-traits (dependencies), serde (dependencies)

## script
Path: `silksurf-extras/servo/components/script`

### dependencies
- accountable-refcell (dependencies; features: none; default-features: true; optional: true; uses: none)
- aes (dependencies; features: none; default-features: true; optional: false; uses: none)
- aes-gcm (dependencies; features: none; default-features: true; optional: false; uses: none)
- aes-kw (dependencies; features: none; default-features: true; optional: false; uses: none)
- app_units (dependencies; features: none; default-features: true; optional: false; uses: none)
- argon2 (dependencies; features: none; default-features: true; optional: false; uses: none)
- arrayvec (dependencies; features: none; default-features: true; optional: false; uses: none)
- atomic_refcell (dependencies; features: none; default-features: true; optional: false; uses: none)
- aws-lc-rs (dependencies; features: none; default-features: true; optional: false; uses: none)
- background_hang_monitor_api (dependencies; features: none; default-features: true; optional: false; uses: none)
- backtrace (dependencies; features: none; default-features: true; optional: false; uses: none)
- base (dependencies; features: none; default-features: true; optional: false; uses: none)
- base64 (dependencies; features: none; default-features: true; optional: false; uses: none)
- base64ct (dependencies; features: none; default-features: true; optional: false; uses: none)
- bincode (dependencies; features: none; default-features: true; optional: false; uses: none)
- bitflags (dependencies; features: none; default-features: true; optional: false; uses: none)
- bluetooth_traits (dependencies; features: none; default-features: true; optional: true; uses: none)
- brotli (dependencies; features: none; default-features: true; optional: false; uses: none)
- canvas_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- cbc (dependencies; features: none; default-features: true; optional: false; uses: none)
- chacha20poly1305 (dependencies; features: none; default-features: true; optional: false; uses: none)
- chardetng (dependencies; features: none; default-features: true; optional: false; uses: none)
- chrono (dependencies; features: none; default-features: true; optional: false; uses: none)
- cipher (dependencies; features: none; default-features: true; optional: false; uses: none)
- compositing_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- constellation_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- content-security-policy (dependencies; features: none; default-features: true; optional: false; uses: none)
- cookie (dependencies; features: none; default-features: true; optional: false; uses: none)
- crossbeam-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- cssparser (dependencies; features: none; default-features: true; optional: false; uses: none)
- ctr (dependencies; features: none; default-features: true; optional: false; uses: none)
- data-url (dependencies; features: none; default-features: true; optional: false; uses: none)
- deny_public_fields (dependencies; features: none; default-features: true; optional: false; uses: none)
- der (dependencies; features: none; default-features: true; optional: false; uses: none)
- devtools_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- digest (dependencies; features: none; default-features: true; optional: false; uses: none)
- dom_struct (dependencies; features: none; default-features: true; optional: false; uses: none)
- domobject_derive (dependencies; features: none; default-features: true; optional: false; uses: none)
- ecdsa (dependencies; features: none; default-features: true; optional: false; uses: none)
- elliptic-curve (dependencies; features: none; default-features: true; optional: false; uses: none)
- embedder_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- encoding_rs (dependencies; features: none; default-features: true; optional: false; uses: none)
- euclid (dependencies; features: none; default-features: true; optional: false; uses: none)
- flate2 (dependencies; features: none; default-features: true; optional: false; uses: none)
- fonts (dependencies; features: none; default-features: true; optional: false; uses: none)
- fonts_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- glow (dependencies; features: none; default-features: true; optional: false; uses: none)
- headers (dependencies; features: none; default-features: true; optional: false; uses: none)
- hkdf (dependencies; features: none; default-features: true; optional: false; uses: none)
- html5ever (dependencies; features: none; default-features: true; optional: false; uses: none)
- http (dependencies; features: none; default-features: true; optional: false; uses: none)
- hyper_serde (dependencies; features: none; default-features: true; optional: false; uses: none)
- image (dependencies; features: none; default-features: true; optional: false; uses: none)
- indexmap (dependencies; features: none; default-features: true; optional: false; uses: none)
- ipc-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- itertools (dependencies; features: none; default-features: true; optional: false; uses: none)
- js (dependencies; features: none; default-features: true; optional: false; uses: none)
- jstraceable_derive (dependencies; features: none; default-features: true; optional: false; uses: none)
- keyboard-types (dependencies; features: none; default-features: true; optional: false; uses: none)
- kurbo (dependencies; features: none; default-features: true; optional: false; uses: none)
- layout_api (dependencies; features: none; default-features: true; optional: false; uses: none)
- libc (dependencies; features: none; default-features: true; optional: false; uses: none)
- log (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of_derive (dependencies; features: none; default-features: true; optional: false; uses: none)
- markup5ever (dependencies; features: none; default-features: true; optional: false; uses: none)
- media (dependencies; features: none; default-features: true; optional: false; uses: none)
- metrics (dependencies; features: none; default-features: true; optional: false; uses: none)
- mime (dependencies; features: none; default-features: true; optional: false; uses: none)
- mime_guess (dependencies; features: none; default-features: true; optional: false; uses: none)
- ml-kem (dependencies; features: none; default-features: true; optional: false; uses: none)
- net_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- nom (dependencies; features: none; default-features: true; optional: false; uses: none)
- nom-rfc8288 (dependencies; features: none; default-features: true; optional: false; uses: none)
- num-bigint-dig (dependencies; features: none; default-features: true; optional: false; uses: none)
- num-traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- num_cpus (dependencies; features: none; default-features: true; optional: false; uses: none)
- p256 (dependencies; features: none; default-features: true; optional: false; uses: none)
- p384 (dependencies; features: none; default-features: true; optional: false; uses: none)
- p521 (dependencies; features: none; default-features: true; optional: false; uses: none)
- parking_lot (dependencies; features: none; default-features: true; optional: false; uses: none)
- percent-encoding (dependencies; features: none; default-features: true; optional: false; uses: none)
- phf (dependencies; features: none; default-features: true; optional: false; uses: none)
- pixels (dependencies; features: none; default-features: true; optional: false; uses: none)
- pkcs8 (dependencies; features: none; default-features: true; optional: false; uses: none)
- profile_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- rand (dependencies; features: none; default-features: true; optional: false; uses: none)
- range (dependencies; features: none; default-features: true; optional: false; uses: none)
- regex (dependencies; features: none; default-features: true; optional: false; uses: none)
- rsa (dependencies; features: none; default-features: true; optional: false; uses: none)
- rustc-hash (dependencies; features: none; default-features: true; optional: false; uses: none)
- script_bindings (dependencies; features: none; default-features: true; optional: false; uses: none)
- script_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- sec1 (dependencies; features: none; default-features: true; optional: false; uses: none)
- selectors (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde (dependencies; features: derive; default-features: true; optional: false; uses: none)
- serde_json (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo-media (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_arc (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_config (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_geometry (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_url (dependencies; features: none; default-features: true; optional: false; uses: none)
- sha1 (dependencies; features: none; default-features: true; optional: false; uses: none)
- sha2 (dependencies; features: none; default-features: true; optional: false; uses: none)
- sha3 (dependencies; features: none; default-features: true; optional: false; uses: none)
- smallvec (dependencies; features: none; default-features: true; optional: false; uses: none)
- storage_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- strum (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo_atoms (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo_dom (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo_malloc_size_of (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- swapper (dependencies; features: none; default-features: true; optional: false; uses: none)
- tempfile (dependencies; features: none; default-features: true; optional: false; uses: none)
- tendril (dependencies; features: none; default-features: true; optional: false; uses: none)
- time (dependencies; features: none; default-features: true; optional: false; uses: none)
- timers (dependencies; features: none; default-features: true; optional: false; uses: none)
- tracing (dependencies; features: none; default-features: true; optional: true; uses: none)
- unicode-bidi (dependencies; features: none; default-features: true; optional: false; uses: none)
- unicode-script (dependencies; features: none; default-features: true; optional: false; uses: none)
- unicode-segmentation (dependencies; features: none; default-features: true; optional: false; uses: none)
- url (dependencies; features: none; default-features: true; optional: false; uses: none)
- urlpattern (dependencies; features: none; default-features: true; optional: false; uses: none)
- utf-8 (dependencies; features: none; default-features: true; optional: false; uses: none)
- uuid (dependencies; features: serde; default-features: true; optional: false; uses: none)
- webdriver (dependencies; features: none; default-features: true; optional: false; uses: none)
- webgpu_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- webrender_api (dependencies; features: none; default-features: true; optional: false; uses: none)
- webxr-api (dependencies; features: ipc; default-features: true; optional: true; uses: none)
- wgpu-core (dependencies; features: none; default-features: true; optional: false; uses: none)
- wgpu-types (dependencies; features: none; default-features: true; optional: false; uses: none)
- x25519-dalek (dependencies; features: none; default-features: true; optional: false; uses: none)
- xml5ever (dependencies; features: none; default-features: true; optional: false; uses: none)
- xpath (dependencies; features: none; default-features: true; optional: false; uses: none)

### target.cfg(not(target_os = "ios")).dependencies
- mozangle (target.cfg(not(target_os = "ios")).dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
accountable-refcell (dependencies), aes (dependencies), aes-gcm (dependencies), aes-kw (dependencies), app_units (dependencies), argon2 (dependencies), arrayvec (dependencies), atomic_refcell (dependencies), aws-lc-rs (dependencies), background_hang_monitor_api (dependencies), backtrace (dependencies), base (dependencies), base64 (dependencies), base64ct (dependencies), bincode (dependencies), bitflags (dependencies), bluetooth_traits (dependencies), brotli (dependencies), canvas_traits (dependencies), cbc (dependencies), chacha20poly1305 (dependencies), chardetng (dependencies), chrono (dependencies), cipher (dependencies), compositing_traits (dependencies), constellation_traits (dependencies), content-security-policy (dependencies), cookie (dependencies), crossbeam-channel (dependencies), cssparser (dependencies), ctr (dependencies), data-url (dependencies), deny_public_fields (dependencies), der (dependencies), devtools_traits (dependencies), digest (dependencies), dom_struct (dependencies), domobject_derive (dependencies), ecdsa (dependencies), elliptic-curve (dependencies), embedder_traits (dependencies), encoding_rs (dependencies), euclid (dependencies), flate2 (dependencies), fonts (dependencies), fonts_traits (dependencies), glow (dependencies), headers (dependencies), hkdf (dependencies), html5ever (dependencies), http (dependencies), hyper_serde (dependencies), image (dependencies), indexmap (dependencies), ipc-channel (dependencies), itertools (dependencies), js (dependencies), jstraceable_derive (dependencies), keyboard-types (dependencies), kurbo (dependencies), layout_api (dependencies), libc (dependencies), log (dependencies), malloc_size_of (dependencies), malloc_size_of_derive (dependencies), markup5ever (dependencies), media (dependencies), metrics (dependencies), mime (dependencies), mime_guess (dependencies), ml-kem (dependencies), mozangle (target.cfg(not(target_os = "ios")).dependencies), net_traits (dependencies), nom (dependencies), nom-rfc8288 (dependencies), num-bigint-dig (dependencies), num-traits (dependencies), num_cpus (dependencies), p256 (dependencies), p384 (dependencies), p521 (dependencies), parking_lot (dependencies), percent-encoding (dependencies), phf (dependencies), pixels (dependencies), pkcs8 (dependencies), profile_traits (dependencies), rand (dependencies), range (dependencies), regex (dependencies), rsa (dependencies), rustc-hash (dependencies), script_bindings (dependencies), script_traits (dependencies), sec1 (dependencies), selectors (dependencies), serde (dependencies), serde_json (dependencies), servo-media (dependencies), servo_arc (dependencies), servo_config (dependencies), servo_geometry (dependencies), servo_url (dependencies), sha1 (dependencies), sha2 (dependencies), sha3 (dependencies), smallvec (dependencies), storage_traits (dependencies), strum (dependencies), stylo (dependencies), stylo_atoms (dependencies), stylo_dom (dependencies), stylo_malloc_size_of (dependencies), stylo_traits (dependencies), swapper (dependencies), tempfile (dependencies), tendril (dependencies), time (dependencies), timers (dependencies), tracing (dependencies), unicode-bidi (dependencies), unicode-script (dependencies), unicode-segmentation (dependencies), url (dependencies), urlpattern (dependencies), utf-8 (dependencies), uuid (dependencies), webdriver (dependencies), webgpu_traits (dependencies), webrender_api (dependencies), webxr-api (dependencies), wgpu-core (dependencies), wgpu-types (dependencies), x25519-dalek (dependencies), xml5ever (dependencies), xpath (dependencies)

## script_bindings
Path: `silksurf-extras/servo/components/script_bindings`

### build-dependencies
- phf_codegen (build-dependencies; features: none; default-features: true; optional: false; uses: build)
- phf_shared (build-dependencies; features: none; default-features: true; optional: false; uses: build)
- serde_json (build-dependencies; features: none; default-features: true; optional: false; uses: build)

### dependencies
- base (dependencies; features: none; default-features: true; optional: false; uses: none)
- bitflags (dependencies; features: none; default-features: true; optional: false; uses: none)
- crossbeam-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- cssparser (dependencies; features: none; default-features: true; optional: false; uses: none)
- deny_public_fields (dependencies; features: none; default-features: true; optional: false; uses: none)
- dom_struct (dependencies; features: none; default-features: true; optional: false; uses: none)
- domobject_derive (dependencies; features: none; default-features: true; optional: false; uses: none)
- html5ever (dependencies; features: none; default-features: true; optional: false; uses: none)
- indexmap (dependencies; features: none; default-features: true; optional: false; uses: none)
- js (dependencies; features: none; default-features: true; optional: false; uses: none)
- jstraceable_derive (dependencies; features: none; default-features: true; optional: false; uses: none)
- keyboard-types (dependencies; features: none; default-features: true; optional: false; uses: none)
- libc (dependencies; features: none; default-features: true; optional: false; uses: none)
- log (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of_derive (dependencies; features: none; default-features: true; optional: false; uses: none)
- num-traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- parking_lot (dependencies; features: none; default-features: true; optional: false; uses: none)
- phf (dependencies; features: none; default-features: true; optional: false; uses: build)
- regex (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_arc (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_config (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_url (dependencies; features: none; default-features: true; optional: false; uses: none)
- smallvec (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo_atoms (dependencies; features: none; default-features: true; optional: false; uses: none)
- tendril (dependencies; features: none; default-features: true; optional: false; uses: none)
- tracing (dependencies; features: none; default-features: true; optional: true; uses: none)
- webxr-api (dependencies; features: none; default-features: true; optional: true; uses: none)
- xml5ever (dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
base (dependencies), bitflags (dependencies), crossbeam-channel (dependencies), cssparser (dependencies), deny_public_fields (dependencies), dom_struct (dependencies), domobject_derive (dependencies), html5ever (dependencies), indexmap (dependencies), js (dependencies), jstraceable_derive (dependencies), keyboard-types (dependencies), libc (dependencies), log (dependencies), malloc_size_of (dependencies), malloc_size_of_derive (dependencies), num-traits (dependencies), parking_lot (dependencies), regex (dependencies), servo_arc (dependencies), servo_config (dependencies), servo_url (dependencies), smallvec (dependencies), stylo (dependencies), stylo_atoms (dependencies), tendril (dependencies), tracing (dependencies), webxr-api (dependencies), xml5ever (dependencies)

## libservo
Path: `silksurf-extras/servo/components/servo`

### dependencies
- background_hang_monitor (dependencies; features: none; default-features: true; optional: false; uses: none)
- base (dependencies; features: none; default-features: true; optional: false; uses: none)
- bincode (dependencies; features: none; default-features: true; optional: false; uses: none)
- bitflags (dependencies; features: none; default-features: true; optional: false; uses: none)
- bluetooth (dependencies; features: none; default-features: true; optional: true; uses: none)
- bluetooth_traits (dependencies; features: none; default-features: true; optional: true; uses: none)
- canvas_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- compositing (dependencies; features: none; default-features: true; optional: false; uses: none)
- compositing_traits (dependencies; features: none; default-features: true; optional: false; uses: tests)
- constellation (dependencies; features: none; default-features: true; optional: false; uses: none)
- constellation_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- crossbeam-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- devtools (dependencies; features: none; default-features: true; optional: false; uses: none)
- devtools_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- dpi (dependencies; features: none; default-features: true; optional: false; uses: tests, examples)
- embedder_traits (dependencies; features: none; default-features: true; optional: false; uses: tests, examples)
- env_logger (dependencies; features: none; default-features: true; optional: false; uses: none)
- euclid (dependencies; features: none; default-features: true; optional: false; uses: tests, examples)
- fonts (dependencies; features: none; default-features: true; optional: false; uses: none)
- gleam (dependencies; features: none; default-features: true; optional: false; uses: none)
- gstreamer (dependencies; features: none; default-features: true; optional: true; uses: none)
- image (dependencies; features: none; default-features: true; optional: false; uses: none)
- ipc-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- keyboard-types (dependencies; features: none; default-features: true; optional: false; uses: none)
- layout (dependencies; features: none; default-features: true; optional: false; uses: none)
- layout_api (dependencies; features: none; default-features: true; optional: false; uses: none)
- log (dependencies; features: none; default-features: true; optional: false; uses: none)
- media (dependencies; features: none; default-features: true; optional: false; uses: none)
- mozangle (dependencies; features: none; default-features: true; optional: false; uses: none)
- net (dependencies; features: none; default-features: true; optional: false; uses: tests)
- net_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- parking_lot (dependencies; features: none; default-features: true; optional: false; uses: none)
- profile (dependencies; features: none; default-features: true; optional: false; uses: none)
- profile_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- rayon (dependencies; features: none; default-features: true; optional: false; uses: none)
- rustc-hash (dependencies; features: none; default-features: true; optional: false; uses: none)
- script (dependencies; features: none; default-features: true; optional: false; uses: none)
- script_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo-media (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo-media-dummy (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo-media-gstreamer (dependencies; features: none; default-features: true; optional: true; uses: none)
- servo-tracing (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_allocator (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_config (dependencies; features: none; default-features: true; optional: false; uses: tests)
- servo_geometry (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_url (dependencies; features: none; default-features: true; optional: false; uses: tests)
- storage (dependencies; features: none; default-features: true; optional: false; uses: none)
- storage_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- surfman (dependencies; features: none; default-features: true; optional: false; uses: none)
- tracing (dependencies; features: none; default-features: true; optional: true; uses: examples)
- url (dependencies; features: none; default-features: true; optional: false; uses: tests, examples)
- webgl (dependencies; features: none; default-features: false; optional: false; uses: none)
- webgpu (dependencies; features: none; default-features: true; optional: false; uses: none)
- webrender (dependencies; features: none; default-features: true; optional: false; uses: none)
- webrender_api (dependencies; features: none; default-features: true; optional: false; uses: tests, examples)
- webxr-api (dependencies; features: none; default-features: true; optional: true; uses: none)

### dev-dependencies
- http (dev-dependencies; features: none; default-features: true; optional: false; uses: none)
- http-body-util (dev-dependencies; features: none; default-features: true; optional: false; uses: tests)
- hyper (dev-dependencies; features: none; default-features: true; optional: false; uses: tests)
- libservo (dev-dependencies; features: tracing; default-features: true; optional: false; uses: none)
- net (dev-dependencies; features: test-util; default-features: true; optional: false; uses: tests)
- rustls (dev-dependencies; features: aws-lc-rs; default-features: false; optional: false; uses: examples)
- tracing (dev-dependencies; features: none; default-features: true; optional: false; uses: examples)
- winit (dev-dependencies; features: none; default-features: true; optional: false; uses: examples)

### target.cfg(all(not(target_os = "windows"), not(target_os = "ios"), not(target_os = "android"), not(target_env = "ohos"), not(target_arch = "arm"), not(target_arch = "aarch64"))).dependencies
- gaol (target.cfg(all(not(target_os = "windows"), not(target_os = "ios"), not(target_os = "android"), not(target_env = "ohos"), not(target_arch = "arm"), not(target_arch = "aarch64"))).dependencies; features: none; default-features: true; optional: false; uses: none)

### target.cfg(any(target_os = "android", target_env = "ohos")).dependencies
- webxr (target.cfg(any(target_os = "android", target_env = "ohos")).dependencies; features: none; default-features: true; optional: true; uses: none)

### target.cfg(not(any(target_os = "android", target_env = "ohos"))).dependencies
- arboard (target.cfg(not(any(target_os = "android", target_env = "ohos"))).dependencies; features: none; default-features: true; optional: true; uses: none)
- webxr (target.cfg(not(any(target_os = "android", target_env = "ohos"))).dependencies; features: ipc, glwindow, headless; default-features: true; optional: false; uses: none)

### target.cfg(target_os = "windows").dependencies
- webxr (target.cfg(target_os = "windows").dependencies; features: ipc, glwindow, headless, openxr-api; default-features: true; optional: false; uses: none)

### Unused (heuristic)
arboard (target.cfg(not(any(target_os = "android", target_env = "ohos"))).dependencies), background_hang_monitor (dependencies), base (dependencies), bincode (dependencies), bitflags (dependencies), bluetooth (dependencies), bluetooth_traits (dependencies), canvas_traits (dependencies), compositing (dependencies), constellation (dependencies), constellation_traits (dependencies), crossbeam-channel (dependencies), devtools (dependencies), devtools_traits (dependencies), env_logger (dependencies), fonts (dependencies), gaol (target.cfg(all(not(target_os = "windows"), not(target_os = "ios"), not(target_os = "android"), not(target_env = "ohos"), not(target_arch = "arm"), not(target_arch = "aarch64"))).dependencies), gleam (dependencies), gstreamer (dependencies), http (dev-dependencies), image (dependencies), ipc-channel (dependencies), keyboard-types (dependencies), layout (dependencies), layout_api (dependencies), libservo (dev-dependencies), log (dependencies), media (dependencies), mozangle (dependencies), net_traits (dependencies), parking_lot (dependencies), profile (dependencies), profile_traits (dependencies), rayon (dependencies), rustc-hash (dependencies), script (dependencies), script_traits (dependencies), serde (dependencies), servo-media (dependencies), servo-media-dummy (dependencies), servo-media-gstreamer (dependencies), servo-tracing (dependencies), servo_allocator (dependencies), servo_geometry (dependencies), storage (dependencies), storage_traits (dependencies), stylo (dependencies), stylo_traits (dependencies), surfman (dependencies), webgl (dependencies), webgpu (dependencies), webrender (dependencies), webxr (target.cfg(any(target_os = "android", target_env = "ohos")).dependencies), webxr (target.cfg(not(any(target_os = "android", target_env = "ohos"))).dependencies), webxr (target.cfg(target_os = "windows").dependencies), webxr-api (dependencies)

## servo-tracing
Path: `silksurf-extras/servo/components/servo_tracing`

### dependencies
- proc-macro2 (dependencies; features: none; default-features: true; optional: false; uses: none)
- quote (dependencies; features: none; default-features: true; optional: false; uses: none)
- syn (dependencies; features: full; default-features: true; optional: false; uses: none)

### dev-dependencies
- prettyplease (dev-dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
prettyplease (dev-dependencies), proc-macro2 (dependencies), quote (dependencies), syn (dependencies)

## background_hang_monitor_api
Path: `silksurf-extras/servo/components/shared/background_hang_monitor`

### dependencies
- base (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde (dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
base (dependencies), serde (dependencies)

## base
Path: `silksurf-extras/servo/components/shared/base`

### dependencies
- crossbeam-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- ipc-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- log (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of_derive (dependencies; features: none; default-features: true; optional: false; uses: none)
- parking_lot (dependencies; features: none; default-features: true; optional: false; uses: none)
- rayon (dependencies; features: none; default-features: true; optional: false; uses: none)
- regex (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde_json (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_config (dependencies; features: none; default-features: true; optional: false; uses: none)
- time (dependencies; features: none; default-features: true; optional: false; uses: none)
- webrender_api (dependencies; features: none; default-features: true; optional: false; uses: none)

### target.cfg(all(unix, not(any(target_os = "macos", target_os = "ios")))).dependencies
- libc (target.cfg(all(unix, not(any(target_os = "macos", target_os = "ios")))).dependencies; features: none; default-features: true; optional: false; uses: none)

### target.cfg(any(target_os = "macos", target_os = "ios")).dependencies
- mach2 (target.cfg(any(target_os = "macos", target_os = "ios")).dependencies; features: none; default-features: true; optional: false; uses: none)

### target.cfg(target_os = "windows").dependencies
- windows-sys (target.cfg(target_os = "windows").dependencies; features: Win32_System_Performance; default-features: true; optional: false; uses: none)

### Unused (heuristic)
crossbeam-channel (dependencies), ipc-channel (dependencies), libc (target.cfg(all(unix, not(any(target_os = "macos", target_os = "ios")))).dependencies), log (dependencies), mach2 (target.cfg(any(target_os = "macos", target_os = "ios")).dependencies), malloc_size_of (dependencies), malloc_size_of_derive (dependencies), parking_lot (dependencies), rayon (dependencies), regex (dependencies), serde (dependencies), serde_json (dependencies), servo_config (dependencies), time (dependencies), webrender_api (dependencies), windows-sys (target.cfg(target_os = "windows").dependencies)

## bluetooth_traits
Path: `silksurf-extras/servo/components/shared/bluetooth`

### dependencies
- base (dependencies; features: none; default-features: true; optional: false; uses: none)
- embedder_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- ipc-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- regex (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde (dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
base (dependencies), embedder_traits (dependencies), ipc-channel (dependencies), regex (dependencies), serde (dependencies)

## canvas_traits
Path: `silksurf-extras/servo/components/shared/canvas`

### dependencies
- base (dependencies; features: none; default-features: true; optional: false; uses: none)
- crossbeam-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- euclid (dependencies; features: none; default-features: true; optional: false; uses: none)
- fonts_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- glow (dependencies; features: none; default-features: true; optional: false; uses: none)
- ipc-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- kurbo (dependencies; features: serde; default-features: true; optional: false; uses: none)
- malloc_size_of (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of_derive (dependencies; features: none; default-features: true; optional: false; uses: none)
- pixels (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_config (dependencies; features: none; default-features: true; optional: false; uses: none)
- strum (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo (dependencies; features: none; default-features: true; optional: false; uses: none)
- webrender_api (dependencies; features: none; default-features: true; optional: false; uses: none)
- webxr-api (dependencies; features: ipc; default-features: true; optional: false; uses: none)

### Unused (heuristic)
base (dependencies), crossbeam-channel (dependencies), euclid (dependencies), fonts_traits (dependencies), glow (dependencies), ipc-channel (dependencies), kurbo (dependencies), malloc_size_of (dependencies), malloc_size_of_derive (dependencies), pixels (dependencies), serde (dependencies), servo_config (dependencies), strum (dependencies), stylo (dependencies), webrender_api (dependencies), webxr-api (dependencies)

## compositing_traits
Path: `silksurf-extras/servo/components/shared/compositing`

### dependencies
- base (dependencies; features: none; default-features: true; optional: false; uses: tests)
- bincode (dependencies; features: none; default-features: true; optional: false; uses: none)
- bitflags (dependencies; features: none; default-features: true; optional: false; uses: none)
- canvas_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- crossbeam-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- dpi (dependencies; features: none; default-features: true; optional: false; uses: none)
- embedder_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- euclid (dependencies; features: none; default-features: true; optional: false; uses: tests)
- gleam (dependencies; features: none; default-features: true; optional: false; uses: none)
- glow (dependencies; features: none; default-features: true; optional: false; uses: none)
- image (dependencies; features: none; default-features: true; optional: false; uses: none)
- ipc-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- log (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of_derive (dependencies; features: none; default-features: true; optional: false; uses: none)
- parking_lot (dependencies; features: none; default-features: true; optional: false; uses: none)
- profile_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- raw-window-handle (dependencies; features: none; default-features: true; optional: false; uses: none)
- rustc-hash (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_geometry (dependencies; features: none; default-features: true; optional: false; uses: none)
- smallvec (dependencies; features: none; default-features: true; optional: false; uses: none)
- strum (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- surfman (dependencies; features: sm-x11; default-features: true; optional: false; uses: none)
- webrender_api (dependencies; features: none; default-features: true; optional: false; uses: tests)

### Unused (heuristic)
bincode (dependencies), bitflags (dependencies), canvas_traits (dependencies), crossbeam-channel (dependencies), dpi (dependencies), embedder_traits (dependencies), gleam (dependencies), glow (dependencies), image (dependencies), ipc-channel (dependencies), log (dependencies), malloc_size_of (dependencies), malloc_size_of_derive (dependencies), parking_lot (dependencies), profile_traits (dependencies), raw-window-handle (dependencies), rustc-hash (dependencies), serde (dependencies), servo_geometry (dependencies), smallvec (dependencies), strum (dependencies), stylo (dependencies), stylo_traits (dependencies), surfman (dependencies)

## constellation_traits
Path: `silksurf-extras/servo/components/shared/constellation`

### dependencies
- base (dependencies; features: none; default-features: true; optional: false; uses: none)
- canvas_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- compositing_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- content-security-policy (dependencies; features: none; default-features: true; optional: false; uses: none)
- devtools_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- embedder_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- encoding_rs (dependencies; features: none; default-features: true; optional: false; uses: none)
- euclid (dependencies; features: none; default-features: true; optional: false; uses: none)
- fonts_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- http (dependencies; features: none; default-features: true; optional: false; uses: none)
- hyper_serde (dependencies; features: none; default-features: true; optional: false; uses: none)
- ipc-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- keyboard-types (dependencies; features: none; default-features: true; optional: false; uses: none)
- log (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of_derive (dependencies; features: none; default-features: true; optional: false; uses: none)
- net_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- pixels (dependencies; features: none; default-features: true; optional: false; uses: none)
- profile_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- rustc-hash (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_config (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_url (dependencies; features: none; default-features: true; optional: false; uses: none)
- storage_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- strum (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- uuid (dependencies; features: none; default-features: true; optional: false; uses: none)
- webgpu_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- webrender_api (dependencies; features: none; default-features: true; optional: false; uses: none)
- wgpu-core (dependencies; features: none; default-features: true; optional: true; uses: none)

### Unused (heuristic)
base (dependencies), canvas_traits (dependencies), compositing_traits (dependencies), content-security-policy (dependencies), devtools_traits (dependencies), embedder_traits (dependencies), encoding_rs (dependencies), euclid (dependencies), fonts_traits (dependencies), http (dependencies), hyper_serde (dependencies), ipc-channel (dependencies), keyboard-types (dependencies), log (dependencies), malloc_size_of (dependencies), malloc_size_of_derive (dependencies), net_traits (dependencies), pixels (dependencies), profile_traits (dependencies), rustc-hash (dependencies), serde (dependencies), servo_config (dependencies), servo_url (dependencies), storage_traits (dependencies), strum (dependencies), stylo_traits (dependencies), uuid (dependencies), webgpu_traits (dependencies), webrender_api (dependencies), wgpu-core (dependencies)

## devtools_traits
Path: `silksurf-extras/servo/components/shared/devtools`

### dependencies
- base (dependencies; features: none; default-features: true; optional: false; uses: none)
- bitflags (dependencies; features: none; default-features: true; optional: false; uses: none)
- embedder_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- http (dependencies; features: none; default-features: true; optional: false; uses: none)
- log (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of_derive (dependencies; features: none; default-features: true; optional: false; uses: none)
- net_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_url (dependencies; features: none; default-features: true; optional: false; uses: none)
- uuid (dependencies; features: serde; default-features: true; optional: false; uses: none)

### Unused (heuristic)
base (dependencies), bitflags (dependencies), embedder_traits (dependencies), http (dependencies), log (dependencies), malloc_size_of (dependencies), malloc_size_of_derive (dependencies), net_traits (dependencies), serde (dependencies), servo_url (dependencies), uuid (dependencies)

## embedder_traits
Path: `silksurf-extras/servo/components/shared/embedder`

### dependencies
- base (dependencies; features: none; default-features: true; optional: false; uses: none)
- bitflags (dependencies; features: none; default-features: true; optional: false; uses: none)
- cookie (dependencies; features: none; default-features: true; optional: false; uses: none)
- crossbeam-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- euclid (dependencies; features: none; default-features: true; optional: false; uses: none)
- http (dependencies; features: none; default-features: true; optional: false; uses: none)
- hyper_serde (dependencies; features: none; default-features: true; optional: false; uses: none)
- image (dependencies; features: none; default-features: true; optional: false; uses: none)
- ipc-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- keyboard-types (dependencies; features: none; default-features: true; optional: false; uses: none)
- log (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of_derive (dependencies; features: none; default-features: true; optional: false; uses: none)
- num-derive (dependencies; features: none; default-features: true; optional: false; uses: none)
- pixels (dependencies; features: none; default-features: true; optional: false; uses: none)
- rustc-hash (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_geometry (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_url (dependencies; features: none; default-features: true; optional: false; uses: none)
- strum (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- url (dependencies; features: none; default-features: true; optional: false; uses: none)
- uuid (dependencies; features: none; default-features: true; optional: false; uses: none)
- webdriver (dependencies; features: none; default-features: true; optional: false; uses: none)
- webrender_api (dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
base (dependencies), bitflags (dependencies), cookie (dependencies), crossbeam-channel (dependencies), euclid (dependencies), http (dependencies), hyper_serde (dependencies), image (dependencies), ipc-channel (dependencies), keyboard-types (dependencies), log (dependencies), malloc_size_of (dependencies), malloc_size_of_derive (dependencies), num-derive (dependencies), pixels (dependencies), rustc-hash (dependencies), serde (dependencies), servo_geometry (dependencies), servo_url (dependencies), strum (dependencies), stylo (dependencies), stylo_traits (dependencies), url (dependencies), uuid (dependencies), webdriver (dependencies), webrender_api (dependencies)

## fonts_traits
Path: `silksurf-extras/servo/components/shared/fonts`

### dependencies
- atomic_refcell (dependencies; features: none; default-features: true; optional: false; uses: none)
- base (dependencies; features: none; default-features: true; optional: false; uses: none)
- ipc-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- log (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of_derive (dependencies; features: none; default-features: true; optional: false; uses: none)
- memmap2 (dependencies; features: none; default-features: true; optional: false; uses: none)
- parking_lot (dependencies; features: none; default-features: true; optional: false; uses: none)
- profile_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- range (dependencies; features: none; default-features: true; optional: false; uses: none)
- read-fonts (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_url (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo (dependencies; features: none; default-features: true; optional: false; uses: none)
- webrender_api (dependencies; features: none; default-features: true; optional: false; uses: none)

### target.cfg(target_os = "windows").dependencies
- dwrote (target.cfg(target_os = "windows").dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
atomic_refcell (dependencies), base (dependencies), dwrote (target.cfg(target_os = "windows").dependencies), ipc-channel (dependencies), log (dependencies), malloc_size_of (dependencies), malloc_size_of_derive (dependencies), memmap2 (dependencies), parking_lot (dependencies), profile_traits (dependencies), range (dependencies), read-fonts (dependencies), serde (dependencies), servo_url (dependencies), stylo (dependencies), webrender_api (dependencies)

## layout_api
Path: `silksurf-extras/servo/components/shared/layout`

### dependencies
- app_units (dependencies; features: none; default-features: true; optional: false; uses: none)
- atomic_refcell (dependencies; features: none; default-features: true; optional: false; uses: none)
- background_hang_monitor_api (dependencies; features: none; default-features: true; optional: false; uses: none)
- base (dependencies; features: none; default-features: true; optional: false; uses: none)
- bitflags (dependencies; features: none; default-features: true; optional: false; uses: none)
- compositing_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- constellation_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- crossbeam-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- embedder_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- euclid (dependencies; features: none; default-features: true; optional: false; uses: none)
- fonts (dependencies; features: none; default-features: true; optional: false; uses: none)
- fonts_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- html5ever (dependencies; features: none; default-features: true; optional: false; uses: none)
- libc (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of_derive (dependencies; features: none; default-features: true; optional: false; uses: none)
- net_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- parking_lot (dependencies; features: none; default-features: true; optional: false; uses: none)
- pixels (dependencies; features: none; default-features: true; optional: false; uses: none)
- profile_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- range (dependencies; features: none; default-features: true; optional: false; uses: none)
- rustc-hash (dependencies; features: none; default-features: true; optional: false; uses: none)
- script_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- selectors (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_arc (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_url (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- webrender_api (dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
app_units (dependencies), atomic_refcell (dependencies), background_hang_monitor_api (dependencies), base (dependencies), bitflags (dependencies), compositing_traits (dependencies), constellation_traits (dependencies), crossbeam-channel (dependencies), embedder_traits (dependencies), euclid (dependencies), fonts (dependencies), fonts_traits (dependencies), html5ever (dependencies), libc (dependencies), malloc_size_of (dependencies), malloc_size_of_derive (dependencies), net_traits (dependencies), parking_lot (dependencies), pixels (dependencies), profile_traits (dependencies), range (dependencies), rustc-hash (dependencies), script_traits (dependencies), selectors (dependencies), serde (dependencies), servo_arc (dependencies), servo_url (dependencies), stylo (dependencies), stylo_traits (dependencies), webrender_api (dependencies)

## net_traits
Path: `silksurf-extras/servo/components/shared/net`

### dependencies
- base (dependencies; features: none; default-features: true; optional: false; uses: tests)
- compositing_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- content-security-policy (dependencies; features: none; default-features: true; optional: false; uses: none)
- cookie (dependencies; features: none; default-features: true; optional: false; uses: none)
- crossbeam-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- data-url (dependencies; features: none; default-features: true; optional: false; uses: none)
- embedder_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- headers (dependencies; features: none; default-features: true; optional: false; uses: none)
- http (dependencies; features: none; default-features: true; optional: false; uses: none)
- hyper-util (dependencies; features: none; default-features: true; optional: false; uses: none)
- hyper_serde (dependencies; features: none; default-features: true; optional: false; uses: none)
- indexmap (dependencies; features: none; default-features: true; optional: false; uses: none)
- ipc-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- log (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of_derive (dependencies; features: none; default-features: true; optional: false; uses: none)
- mime (dependencies; features: none; default-features: true; optional: false; uses: tests)
- num-traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- parking_lot (dependencies; features: none; default-features: true; optional: false; uses: none)
- percent-encoding (dependencies; features: none; default-features: true; optional: false; uses: none)
- pixels (dependencies; features: none; default-features: true; optional: false; uses: none)
- profile_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- rand (dependencies; features: none; default-features: true; optional: false; uses: none)
- rustc-hash (dependencies; features: none; default-features: true; optional: false; uses: none)
- rustls-pki-types (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_arc (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_url (dependencies; features: none; default-features: true; optional: false; uses: none)
- tower (dependencies; features: none; default-features: true; optional: false; uses: none)
- url (dependencies; features: none; default-features: true; optional: false; uses: none)
- uuid (dependencies; features: none; default-features: true; optional: false; uses: none)
- webrender_api (dependencies; features: none; default-features: true; optional: false; uses: none)

### dev-dependencies
- embedder_traits (dev-dependencies; features: baked-default-resources; default-features: true; optional: false; uses: none)

### Unused (heuristic)
compositing_traits (dependencies), content-security-policy (dependencies), cookie (dependencies), crossbeam-channel (dependencies), data-url (dependencies), embedder_traits (dependencies), embedder_traits (dev-dependencies), headers (dependencies), http (dependencies), hyper-util (dependencies), hyper_serde (dependencies), indexmap (dependencies), ipc-channel (dependencies), log (dependencies), malloc_size_of (dependencies), malloc_size_of_derive (dependencies), num-traits (dependencies), parking_lot (dependencies), percent-encoding (dependencies), pixels (dependencies), profile_traits (dependencies), rand (dependencies), rustc-hash (dependencies), rustls-pki-types (dependencies), serde (dependencies), servo_arc (dependencies), servo_url (dependencies), tower (dependencies), url (dependencies), uuid (dependencies), webrender_api (dependencies)

## profile_traits
Path: `silksurf-extras/servo/components/shared/profile`

### dependencies
- base (dependencies; features: none; default-features: true; optional: false; uses: none)
- crossbeam-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- ipc-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- log (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of_derive (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_allocator (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_config (dependencies; features: none; default-features: true; optional: false; uses: none)
- time (dependencies; features: none; default-features: true; optional: false; uses: none)
- tracing (dependencies; features: none; default-features: true; optional: true; uses: none)

### Unused (heuristic)
base (dependencies), crossbeam-channel (dependencies), ipc-channel (dependencies), log (dependencies), malloc_size_of (dependencies), malloc_size_of_derive (dependencies), serde (dependencies), servo_allocator (dependencies), servo_config (dependencies), time (dependencies), tracing (dependencies)

## script_traits
Path: `silksurf-extras/servo/components/shared/script`

### dependencies
- background_hang_monitor_api (dependencies; features: none; default-features: true; optional: false; uses: none)
- base (dependencies; features: none; default-features: true; optional: false; uses: none)
- bluetooth_traits (dependencies; features: none; default-features: true; optional: true; uses: none)
- canvas_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- compositing_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- constellation_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- content-security-policy (dependencies; features: none; default-features: true; optional: false; uses: none)
- crossbeam-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- devtools_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- embedder_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- euclid (dependencies; features: none; default-features: true; optional: false; uses: none)
- fonts_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- ipc-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- keyboard-types (dependencies; features: none; default-features: true; optional: false; uses: none)
- log (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of_derive (dependencies; features: none; default-features: true; optional: false; uses: none)
- media (dependencies; features: none; default-features: true; optional: false; uses: none)
- net_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- pixels (dependencies; features: none; default-features: true; optional: false; uses: none)
- profile_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- rustc-hash (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_config (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_url (dependencies; features: none; default-features: true; optional: false; uses: none)
- storage_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- strum (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo_atoms (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- webgpu_traits (dependencies; features: none; default-features: true; optional: true; uses: none)
- webrender_api (dependencies; features: none; default-features: true; optional: false; uses: none)
- webxr-api (dependencies; features: ipc; default-features: true; optional: false; uses: none)

### Unused (heuristic)
background_hang_monitor_api (dependencies), base (dependencies), bluetooth_traits (dependencies), canvas_traits (dependencies), compositing_traits (dependencies), constellation_traits (dependencies), content-security-policy (dependencies), crossbeam-channel (dependencies), devtools_traits (dependencies), embedder_traits (dependencies), euclid (dependencies), fonts_traits (dependencies), ipc-channel (dependencies), keyboard-types (dependencies), log (dependencies), malloc_size_of (dependencies), malloc_size_of_derive (dependencies), media (dependencies), net_traits (dependencies), pixels (dependencies), profile_traits (dependencies), rustc-hash (dependencies), serde (dependencies), servo_config (dependencies), servo_url (dependencies), storage_traits (dependencies), strum (dependencies), stylo_atoms (dependencies), stylo_traits (dependencies), webgpu_traits (dependencies), webrender_api (dependencies), webxr-api (dependencies)

## storage_traits
Path: `silksurf-extras/servo/components/shared/storage`

### dependencies
- base (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of_derive (dependencies; features: none; default-features: true; optional: false; uses: none)
- profile_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_url (dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
base (dependencies), malloc_size_of (dependencies), malloc_size_of_derive (dependencies), profile_traits (dependencies), serde (dependencies), servo_url (dependencies)

## webgpu_traits
Path: `silksurf-extras/servo/components/shared/webgpu`

### dependencies
- arrayvec (dependencies; features: none; default-features: true; optional: false; uses: none)
- base (dependencies; features: none; default-features: true; optional: false; uses: none)
- ipc-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of (dependencies; features: none; default-features: true; optional: false; uses: none)
- pixels (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde (dependencies; features: none; default-features: true; optional: false; uses: none)
- webrender_api (dependencies; features: none; default-features: true; optional: false; uses: none)
- wgpu-core (dependencies; features: serde, wgsl; default-features: true; optional: false; uses: none)
- wgpu-types (dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
arrayvec (dependencies), base (dependencies), ipc-channel (dependencies), malloc_size_of (dependencies), pixels (dependencies), serde (dependencies), webrender_api (dependencies), wgpu-core (dependencies), wgpu-types (dependencies)

## webxr-api
Path: `silksurf-extras/servo/components/shared/webxr`

### dependencies
- embedder_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- euclid (dependencies; features: none; default-features: true; optional: false; uses: none)
- ipc-channel (dependencies; features: none; default-features: true; optional: true; uses: none)
- log (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde (dependencies; features: none; default-features: true; optional: true; uses: none)

### Unused (heuristic)
embedder_traits (dependencies), euclid (dependencies), ipc-channel (dependencies), log (dependencies), serde (dependencies)

## storage
Path: `silksurf-extras/servo/components/storage`

### dependencies
- base (dependencies; features: none; default-features: true; optional: false; uses: tests)
- bincode (dependencies; features: none; default-features: true; optional: false; uses: none)
- libc (dependencies; features: none; default-features: true; optional: false; uses: none)
- log (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of_derive (dependencies; features: none; default-features: true; optional: false; uses: none)
- profile_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- rusqlite (dependencies; features: bundled; default-features: true; optional: false; uses: none)
- rustc-hash (dependencies; features: none; default-features: true; optional: false; uses: none)
- sea-query (dependencies; features: none; default-features: true; optional: false; uses: none)
- sea-query-rusqlite (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde_json (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_config (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_url (dependencies; features: none; default-features: true; optional: false; uses: tests)
- storage_traits (dependencies; features: none; default-features: true; optional: false; uses: tests)
- tempfile (dependencies; features: none; default-features: true; optional: false; uses: tests)
- tokio (dependencies; features: macros, rt-multi-thread, sync; default-features: true; optional: false; uses: none)
- tokio-rustls (dependencies; features: none; default-features: true; optional: false; uses: none)
- tokio-stream (dependencies; features: none; default-features: true; optional: false; uses: none)
- tokio-util (dependencies; features: codec, io; default-features: false; optional: false; uses: none)
- uuid (dependencies; features: none; default-features: true; optional: false; uses: none)

### dev-dependencies
- profile (dev-dependencies; features: none; default-features: true; optional: false; uses: tests)
- url (dev-dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
bincode (dependencies), libc (dependencies), log (dependencies), malloc_size_of (dependencies), malloc_size_of_derive (dependencies), profile_traits (dependencies), rusqlite (dependencies), rustc-hash (dependencies), sea-query (dependencies), sea-query-rusqlite (dependencies), serde (dependencies), serde_json (dependencies), servo_config (dependencies), tokio (dependencies), tokio-rustls (dependencies), tokio-stream (dependencies), tokio-util (dependencies), url (dev-dependencies), uuid (dependencies)

## timers
Path: `silksurf-extras/servo/components/timers`

### dependencies
- crossbeam-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of_derive (dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
crossbeam-channel (dependencies), malloc_size_of (dependencies), malloc_size_of_derive (dependencies)

## servo_url
Path: `silksurf-extras/servo/components/url`

### dependencies
- encoding_rs (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of_derive (dependencies; features: none; default-features: true; optional: false; uses: none)
- rand (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde (dependencies; features: derive; default-features: true; optional: false; uses: none)
- servo_arc (dependencies; features: none; default-features: true; optional: false; uses: none)
- url (dependencies; features: serde; default-features: true; optional: false; uses: tests)
- uuid (dependencies; features: serde; default-features: true; optional: false; uses: none)

### Unused (heuristic)
encoding_rs (dependencies), malloc_size_of (dependencies), malloc_size_of_derive (dependencies), rand (dependencies), serde (dependencies), servo_arc (dependencies), uuid (dependencies)

## webdriver_server
Path: `silksurf-extras/servo/components/webdriver_server`

### dependencies
- base (dependencies; features: none; default-features: true; optional: false; uses: none)
- base64 (dependencies; features: none; default-features: true; optional: false; uses: none)
- cookie (dependencies; features: none; default-features: true; optional: false; uses: none)
- crossbeam-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- embedder_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- euclid (dependencies; features: none; default-features: true; optional: false; uses: none)
- http (dependencies; features: none; default-features: true; optional: false; uses: none)
- image (dependencies; features: none; default-features: true; optional: false; uses: none)
- keyboard-types (dependencies; features: none; default-features: true; optional: false; uses: none)
- log (dependencies; features: none; default-features: true; optional: false; uses: none)
- pixels (dependencies; features: none; default-features: true; optional: false; uses: none)
- rustc-hash (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde_json (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_config (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_geometry (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_url (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- time (dependencies; features: none; default-features: true; optional: false; uses: none)
- uuid (dependencies; features: none; default-features: true; optional: false; uses: none)
- webdriver (dependencies; features: none; default-features: true; optional: false; uses: none)
- webrender_api (dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
base (dependencies), base64 (dependencies), cookie (dependencies), crossbeam-channel (dependencies), embedder_traits (dependencies), euclid (dependencies), http (dependencies), image (dependencies), keyboard-types (dependencies), log (dependencies), pixels (dependencies), rustc-hash (dependencies), serde (dependencies), serde_json (dependencies), servo_config (dependencies), servo_geometry (dependencies), servo_url (dependencies), stylo_traits (dependencies), time (dependencies), uuid (dependencies), webdriver (dependencies), webrender_api (dependencies)

## webgl
Path: `silksurf-extras/servo/components/webgl`

### dependencies
- base (dependencies; features: none; default-features: true; optional: false; uses: none)
- bitflags (dependencies; features: none; default-features: true; optional: false; uses: none)
- byteorder (dependencies; features: none; default-features: true; optional: false; uses: none)
- canvas_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- compositing_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- crossbeam-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- euclid (dependencies; features: none; default-features: true; optional: false; uses: none)
- glow (dependencies; features: none; default-features: true; optional: false; uses: none)
- half (dependencies; features: none; default-features: true; optional: false; uses: none)
- ipc-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- itertools (dependencies; features: none; default-features: true; optional: false; uses: none)
- log (dependencies; features: none; default-features: true; optional: false; uses: none)
- parking_lot (dependencies; features: none; default-features: true; optional: false; uses: none)
- pixels (dependencies; features: none; default-features: true; optional: false; uses: none)
- rustc-hash (dependencies; features: none; default-features: true; optional: false; uses: none)
- surfman (dependencies; features: none; default-features: true; optional: false; uses: none)
- webrender (dependencies; features: none; default-features: true; optional: false; uses: none)
- webrender_api (dependencies; features: none; default-features: true; optional: false; uses: none)
- webxr (dependencies; features: ipc; default-features: true; optional: true; uses: none)
- webxr-api (dependencies; features: ipc; default-features: true; optional: true; uses: none)

### Unused (heuristic)
base (dependencies), bitflags (dependencies), byteorder (dependencies), canvas_traits (dependencies), compositing_traits (dependencies), crossbeam-channel (dependencies), euclid (dependencies), glow (dependencies), half (dependencies), ipc-channel (dependencies), itertools (dependencies), log (dependencies), parking_lot (dependencies), pixels (dependencies), rustc-hash (dependencies), surfman (dependencies), webrender (dependencies), webrender_api (dependencies), webxr (dependencies), webxr-api (dependencies)

## webgpu
Path: `silksurf-extras/servo/components/webgpu`

### dependencies
- arrayvec (dependencies; features: serde; default-features: true; optional: false; uses: none)
- base (dependencies; features: none; default-features: true; optional: false; uses: none)
- compositing_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- euclid (dependencies; features: none; default-features: true; optional: false; uses: none)
- ipc-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- log (dependencies; features: none; default-features: true; optional: false; uses: none)
- pixels (dependencies; features: none; default-features: true; optional: false; uses: none)
- rustc-hash (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde (dependencies; features: serde_derive; default-features: true; optional: false; uses: none)
- servo_config (dependencies; features: none; default-features: true; optional: false; uses: none)
- webgpu_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- webrender_api (dependencies; features: none; default-features: true; optional: false; uses: none)
- wgpu-core (dependencies; features: serde, wgsl; default-features: true; optional: false; uses: none)
- wgpu-types (dependencies; features: none; default-features: true; optional: false; uses: none)

### target.cfg(any(target_os = "ios", target_os = "macos")).dependencies
- wgpu-core (target.cfg(any(target_os = "ios", target_os = "macos")).dependencies; features: metal; default-features: true; optional: false; uses: none)

### target.cfg(any(windows, all(unix, not(any(target_os = "macos", target_os = "ios"))))).dependencies
- wgpu-core (target.cfg(any(windows, all(unix, not(any(target_os = "macos", target_os = "ios"))))).dependencies; features: gles, vulkan; default-features: true; optional: false; uses: none)

### target.cfg(windows).dependencies
- wgpu-core (target.cfg(windows).dependencies; features: dx12, vulkan; default-features: true; optional: false; uses: none)

### Unused (heuristic)
arrayvec (dependencies), base (dependencies), compositing_traits (dependencies), euclid (dependencies), ipc-channel (dependencies), log (dependencies), pixels (dependencies), rustc-hash (dependencies), serde (dependencies), servo_config (dependencies), webgpu_traits (dependencies), webrender_api (dependencies), wgpu-core (dependencies), wgpu-core (target.cfg(any(target_os = "ios", target_os = "macos")).dependencies), wgpu-core (target.cfg(any(windows, all(unix, not(any(target_os = "macos", target_os = "ios"))))).dependencies), wgpu-core (target.cfg(windows).dependencies), wgpu-types (dependencies)

## webxr
Path: `silksurf-extras/servo/components/webxr`

### dependencies
- crossbeam-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- euclid (dependencies; features: none; default-features: true; optional: false; uses: none)
- glow (dependencies; features: none; default-features: true; optional: false; uses: none)
- log (dependencies; features: none; default-features: true; optional: false; uses: none)
- openxr (dependencies; features: none; default-features: true; optional: true; uses: none)
- raw-window-handle (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde (dependencies; features: none; default-features: true; optional: true; uses: none)
- surfman (dependencies; features: chains, sm-raw-window-handle-06; default-features: true; optional: false; uses: none)
- webxr-api (dependencies; features: none; default-features: true; optional: false; uses: none)

### target.cfg(target_os = "windows").dependencies
- winapi (target.cfg(target_os = "windows").dependencies; features: d3d11, dxgi, winerror; default-features: true; optional: true; uses: none)
- wio (target.cfg(target_os = "windows").dependencies; features: none; default-features: true; optional: true; uses: none)

### Unused (heuristic)
crossbeam-channel (dependencies), euclid (dependencies), glow (dependencies), log (dependencies), openxr (dependencies), raw-window-handle (dependencies), serde (dependencies), surfman (dependencies), webxr-api (dependencies), winapi (target.cfg(target_os = "windows").dependencies), wio (target.cfg(target_os = "windows").dependencies)

## xpath
Path: `silksurf-extras/servo/components/xpath`

### dependencies
- log (dependencies; features: none; default-features: true; optional: false; uses: src)
- malloc_size_of (dependencies; features: none; default-features: true; optional: false; uses: none)
- malloc_size_of_derive (dependencies; features: none; default-features: true; optional: false; uses: src)
- markup5ever (dependencies; features: none; default-features: true; optional: false; uses: src)

### Unused (heuristic)
malloc_size_of (dependencies)

## servoshell
Path: `silksurf-extras/servo/ports/servoshell`

### build-dependencies
- cc (build-dependencies; features: none; default-features: true; optional: false; uses: build)

### dependencies
- bpaf (dependencies; features: derive; default-features: true; optional: false; uses: none)
- cfg-if (dependencies; features: none; default-features: true; optional: false; uses: none)
- crossbeam-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- dpi (dependencies; features: none; default-features: true; optional: false; uses: none)
- euclid (dependencies; features: none; default-features: true; optional: false; uses: none)
- hitrace (dependencies; features: none; default-features: true; optional: true; uses: none)
- image (dependencies; features: none; default-features: true; optional: false; uses: none)
- ipc-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- keyboard-types (dependencies; features: none; default-features: true; optional: false; uses: none)
- libc (dependencies; features: none; default-features: true; optional: false; uses: none)
- libservo (dependencies; features: background_hang_monitor, bluetooth, testbinding, vello_cpu; default-features: false; optional: false; uses: none)
- log (dependencies; features: none; default-features: true; optional: false; uses: none)
- mime_guess (dependencies; features: none; default-features: true; optional: false; uses: none)
- raw-window-handle (dependencies; features: none; default-features: true; optional: false; uses: none)
- rustls (dependencies; features: aws-lc-rs; default-features: true; optional: false; uses: none)
- tokio (dependencies; features: none; default-features: true; optional: false; uses: none)
- tracing (dependencies; features: none; default-features: true; optional: true; uses: none)
- tracing-perfetto (dependencies; features: none; default-features: true; optional: true; uses: none)
- tracing-subscriber (dependencies; features: env-filter; default-features: true; optional: true; uses: none)
- url (dependencies; features: none; default-features: true; optional: false; uses: none)
- webdriver_server (dependencies; features: none; default-features: true; optional: false; uses: none)

### target.cfg(any(all(target_os = "linux", not(target_env = "ohos")), target_os = "macos")).dependencies
- sig (target.cfg(any(all(target_os = "linux", not(target_env = "ohos")), target_os = "macos")).dependencies; features: none; default-features: true; optional: false; uses: none)

### target.cfg(any(target_os = "android", target_env = "ohos")).dependencies
- nix (target.cfg(any(target_os = "android", target_env = "ohos")).dependencies; features: fs; default-features: true; optional: false; uses: none)
- serde_json (target.cfg(any(target_os = "android", target_env = "ohos")).dependencies; features: none; default-features: true; optional: false; uses: none)
- surfman (target.cfg(any(target_os = "android", target_env = "ohos")).dependencies; features: sm-angle-default; default-features: true; optional: false; uses: none)

### target.cfg(not(any(target_os = "android", target_env = "ohos"))).dependencies
- dirs (target.cfg(not(any(target_os = "android", target_env = "ohos"))).dependencies; features: none; default-features: true; optional: false; uses: none)
- egui (target.cfg(not(any(target_os = "android", target_env = "ohos"))).dependencies; features: accesskit; default-features: true; optional: false; uses: none)
- egui-file-dialog (target.cfg(not(any(target_os = "android", target_env = "ohos"))).dependencies; features: none; default-features: true; optional: false; uses: none)
- egui-winit (target.cfg(not(any(target_os = "android", target_env = "ohos"))).dependencies; features: accesskit, clipboard, wayland; default-features: false; optional: false; uses: none)
- egui_glow (target.cfg(not(any(target_os = "android", target_env = "ohos"))).dependencies; features: winit; default-features: true; optional: false; uses: none)
- gilrs (target.cfg(not(any(target_os = "android", target_env = "ohos"))).dependencies; features: none; default-features: true; optional: false; uses: none)
- glow (target.cfg(not(any(target_os = "android", target_env = "ohos"))).dependencies; features: none; default-features: true; optional: false; uses: none)
- headers (target.cfg(not(any(target_os = "android", target_env = "ohos"))).dependencies; features: none; default-features: true; optional: false; uses: none)
- serde_json (target.cfg(not(any(target_os = "android", target_env = "ohos"))).dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_allocator (target.cfg(not(any(target_os = "android", target_env = "ohos"))).dependencies; features: none; default-features: true; optional: false; uses: none)
- surfman (target.cfg(not(any(target_os = "android", target_env = "ohos"))).dependencies; features: sm-raw-window-handle-06, sm-x11; default-features: true; optional: false; uses: none)
- winit (target.cfg(not(any(target_os = "android", target_env = "ohos"))).dependencies; features: none; default-features: true; optional: false; uses: none)

### target.cfg(not(target_os = "android")).dependencies
- backtrace (target.cfg(not(target_os = "android")).dependencies; features: none; default-features: true; optional: false; uses: none)

### target.cfg(target_env = "ohos").dependencies
- env_filter (target.cfg(target_env = "ohos").dependencies; features: none; default-features: true; optional: false; uses: none)
- euclid (target.cfg(target_env = "ohos").dependencies; features: none; default-features: true; optional: false; uses: none)
- hilog (target.cfg(target_env = "ohos").dependencies; features: none; default-features: true; optional: false; uses: none)
- ipc-channel (target.cfg(target_env = "ohos").dependencies; features: force-inprocess; default-features: true; optional: false; uses: none)
- napi-derive-ohos (target.cfg(target_env = "ohos").dependencies; features: none; default-features: true; optional: false; uses: none)
- napi-ohos (target.cfg(target_env = "ohos").dependencies; features: none; default-features: true; optional: false; uses: none)
- ohos-abilitykit-sys (target.cfg(target_env = "ohos").dependencies; features: api-14; default-features: true; optional: false; uses: none)
- ohos-deviceinfo (target.cfg(target_env = "ohos").dependencies; features: none; default-features: true; optional: false; uses: none)
- ohos-ime (target.cfg(target_env = "ohos").dependencies; features: none; default-features: true; optional: false; uses: none)
- ohos-ime-sys (target.cfg(target_env = "ohos").dependencies; features: none; default-features: true; optional: false; uses: none)
- ohos-vsync (target.cfg(target_env = "ohos").dependencies; features: none; default-features: true; optional: false; uses: none)
- ohos-window-manager-sys (target.cfg(target_env = "ohos").dependencies; features: api-14; default-features: true; optional: false; uses: none)
- xcomponent-sys (target.cfg(target_env = "ohos").dependencies; features: api-14, keyboard-types; default-features: true; optional: false; uses: none)

### target.cfg(target_os = "android").dependencies
- android_logger (target.cfg(target_os = "android").dependencies; features: none; default-features: true; optional: false; uses: none)
- jni (target.cfg(target_os = "android").dependencies; features: none; default-features: true; optional: false; uses: none)

### target.cfg(target_os = "macos").dependencies
- objc2-app-kit (target.cfg(target_os = "macos").dependencies; features: std, NSColorSpace, NSResponder, NSView, NSWindow; default-features: false; optional: false; uses: none)
- objc2-foundation (target.cfg(target_os = "macos").dependencies; features: std; default-features: false; optional: false; uses: none)

### target.cfg(target_os = "windows").dependencies
- libservo (target.cfg(target_os = "windows").dependencies; features: no-wgl; default-features: true; optional: false; uses: none)
- windows-sys (target.cfg(target_os = "windows").dependencies; features: Win32_Graphics_Gdi, Win32_System_Console; default-features: true; optional: false; uses: none)

### target.cfg(windows).build-dependencies
- winresource (target.cfg(windows).build-dependencies; features: none; default-features: true; optional: false; uses: build)

### Unused (heuristic)
android_logger (target.cfg(target_os = "android").dependencies), backtrace (target.cfg(not(target_os = "android")).dependencies), bpaf (dependencies), cfg-if (dependencies), crossbeam-channel (dependencies), dirs (target.cfg(not(any(target_os = "android", target_env = "ohos"))).dependencies), dpi (dependencies), egui (target.cfg(not(any(target_os = "android", target_env = "ohos"))).dependencies), egui-file-dialog (target.cfg(not(any(target_os = "android", target_env = "ohos"))).dependencies), egui-winit (target.cfg(not(any(target_os = "android", target_env = "ohos"))).dependencies), egui_glow (target.cfg(not(any(target_os = "android", target_env = "ohos"))).dependencies), env_filter (target.cfg(target_env = "ohos").dependencies), euclid (dependencies), euclid (target.cfg(target_env = "ohos").dependencies), gilrs (target.cfg(not(any(target_os = "android", target_env = "ohos"))).dependencies), glow (target.cfg(not(any(target_os = "android", target_env = "ohos"))).dependencies), headers (target.cfg(not(any(target_os = "android", target_env = "ohos"))).dependencies), hilog (target.cfg(target_env = "ohos").dependencies), hitrace (dependencies), image (dependencies), ipc-channel (dependencies), ipc-channel (target.cfg(target_env = "ohos").dependencies), jni (target.cfg(target_os = "android").dependencies), keyboard-types (dependencies), libc (dependencies), libservo (dependencies), libservo (target.cfg(target_os = "windows").dependencies), log (dependencies), mime_guess (dependencies), napi-derive-ohos (target.cfg(target_env = "ohos").dependencies), napi-ohos (target.cfg(target_env = "ohos").dependencies), nix (target.cfg(any(target_os = "android", target_env = "ohos")).dependencies), objc2-app-kit (target.cfg(target_os = "macos").dependencies), objc2-foundation (target.cfg(target_os = "macos").dependencies), ohos-abilitykit-sys (target.cfg(target_env = "ohos").dependencies), ohos-deviceinfo (target.cfg(target_env = "ohos").dependencies), ohos-ime (target.cfg(target_env = "ohos").dependencies), ohos-ime-sys (target.cfg(target_env = "ohos").dependencies), ohos-vsync (target.cfg(target_env = "ohos").dependencies), ohos-window-manager-sys (target.cfg(target_env = "ohos").dependencies), raw-window-handle (dependencies), rustls (dependencies), serde_json (target.cfg(any(target_os = "android", target_env = "ohos")).dependencies), serde_json (target.cfg(not(any(target_os = "android", target_env = "ohos"))).dependencies), servo_allocator (target.cfg(not(any(target_os = "android", target_env = "ohos"))).dependencies), sig (target.cfg(any(all(target_os = "linux", not(target_env = "ohos")), target_os = "macos")).dependencies), surfman (target.cfg(any(target_os = "android", target_env = "ohos")).dependencies), surfman (target.cfg(not(any(target_os = "android", target_env = "ohos"))).dependencies), tokio (dependencies), tracing (dependencies), tracing-perfetto (dependencies), tracing-subscriber (dependencies), url (dependencies), webdriver_server (dependencies), windows-sys (target.cfg(target_os = "windows").dependencies), winit (target.cfg(not(any(target_os = "android", target_env = "ohos"))).dependencies), xcomponent-sys (target.cfg(target_env = "ohos").dependencies)

## test
Path: `silksurf-extras/servo/python/tidy/tests`

### dependencies
- test-package (dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
test-package (dependencies)

## crown
Path: `silksurf-extras/servo/support/crown`

### dev-dependencies
- compiletest_rs (dev-dependencies; features: tmp; default-features: true; optional: false; uses: none)

### Unused (heuristic)
compiletest_rs (dev-dependencies)

## deny_public_fields_tests
Path: `silksurf-extras/servo/tests/unit/deny_public_fields`

### dependencies
- deny_public_fields (dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
deny_public_fields (dependencies)

## malloc_size_of_tests
Path: `silksurf-extras/servo/tests/unit/malloc_size_of`

### dependencies
- malloc_size_of (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_arc (dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
malloc_size_of (dependencies), servo_arc (dependencies)

## profile_tests
Path: `silksurf-extras/servo/tests/unit/profile`

### dependencies
- ipc-channel (dependencies; features: none; default-features: true; optional: false; uses: none)
- profile (dependencies; features: none; default-features: true; optional: false; uses: none)
- profile_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_config (dependencies; features: none; default-features: true; optional: false; uses: none)
- time (dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
ipc-channel (dependencies), profile (dependencies), profile_traits (dependencies), servo_config (dependencies), time (dependencies)

## script_tests
Path: `silksurf-extras/servo/tests/unit/script`

### dependencies
- base (dependencies; features: none; default-features: true; optional: false; uses: none)
- encoding_rs (dependencies; features: none; default-features: true; optional: false; uses: none)
- euclid (dependencies; features: none; default-features: true; optional: false; uses: none)
- keyboard-types (dependencies; features: none; default-features: true; optional: false; uses: none)
- script (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_url (dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
base (dependencies), encoding_rs (dependencies), euclid (dependencies), keyboard-types (dependencies), script (dependencies), servo_url (dependencies)

## style_tests
Path: `silksurf-extras/servo/tests/unit/style`

### dependencies
- app_units (dependencies; features: none; default-features: true; optional: false; uses: none)
- cssparser (dependencies; features: none; default-features: true; optional: false; uses: none)
- euclid (dependencies; features: none; default-features: true; optional: false; uses: none)
- html5ever (dependencies; features: none; default-features: true; optional: false; uses: none)
- rayon (dependencies; features: none; default-features: true; optional: false; uses: none)
- selectors (dependencies; features: none; default-features: true; optional: false; uses: none)
- serde_json (dependencies; features: none; default-features: true; optional: false; uses: none)
- servo_arc (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo_atoms (dependencies; features: none; default-features: true; optional: false; uses: none)
- stylo_traits (dependencies; features: none; default-features: true; optional: false; uses: none)
- url (dependencies; features: none; default-features: true; optional: false; uses: none)

### Unused (heuristic)
app_units (dependencies), cssparser (dependencies), euclid (dependencies), html5ever (dependencies), rayon (dependencies), selectors (dependencies), serde_json (dependencies), servo_arc (dependencies), stylo (dependencies), stylo_atoms (dependencies), stylo_traits (dependencies), url (dependencies)

## blurmac
Path: `silksurf-extras/servo/third_party/blurmac`

### dependencies
- log (dependencies; features: none; default-features: true; optional: false; uses: src)
- objc2 (dependencies; features: none; default-features: true; optional: false; uses: src)
