# Legacy C Porting Map

AD-024 (Legacy C Tree Retirement, `docs/design/ARCHITECTURE-DECISIONS.md`)
retires the C implementation under `src/`, `include/`, and
`CMakeLists.txt`. This document records which Rust crate owns each C
module's responsibility, so the C tree can be removed without capability
loss and so readers of git history can locate the successor code.

The Rust crates are not ports of the C code. They are independent
implementations built on the cleanroom process (`docs/CLEANROOM.md`);
this map records ownership of responsibility, not lineage of code.

## Module ownership

| C module | Responsibility | Owning Rust crate | Status |
|---|---|---|---|
| `src/document/html_tokenizer.c` | HTML tokenization | `silksurf-html` (html5ever integration, `treesink.rs`) | Rust owns; C deleted (AD-024 executed) |
| `src/document/tree_builder.c` | HTML tree construction | `silksurf-html` | Deleted (AD-024 step 1 executed) |
| `src/document/dom_node.c`, `document.c` | DOM node and document model | `silksurf-dom` | Rust owns; C deleted (AD-024 executed) |
| `src/css/css_tokenizer.c`, `css_parser.c`, `selector.c` | CSS tokenize/parse/select | `silksurf-css` | Rust owns; C deleted (AD-024 executed) |
| `src/document/css_engine.c`, `css_cascade.c`, `css_property_spec.c`, `css_selector_match.c`, `css_select_handler.c`, `css_native_bridge.c` | Cascade and computed style | `silksurf-css` (cascade, `style.rs`) | Rust owns; C deleted (AD-024 executed) |
| `src/layout/box_model.c`, `inline.c` | Box model and inline layout | `silksurf-layout` (Taffy flex/grid, `taffy_layout.rs`) | Rust owns; C deleted (AD-024 executed) |
| `src/rendering/paint.c`, `renderer.c`, `pixel_ops.c` | Paint and rasterization | `silksurf-render` (display list, tiny-skia, SIMD fills) | Rust owns; C deleted (AD-024 executed) |
| `src/rendering/damage_tracker.c` | Damage region tracking (AD-007) | `silksurf-render`/`silksurf-engine` (incremental render path, dirty-node sets) | Rust owns via incremental pipeline; C deleted (AD-024 executed) |
| `src/rendering/pixmap_cache.c` | Pixmap caching | `silksurf-render` | Rust owns; C deleted (AD-024 executed) |
| `src/memory/arena.c`, `pool.c`, `refcount.c` | Arena/pool allocation (AD-004) | `silksurf-core` (`SilkArena`) | Rust owns; C deleted (AD-024 executed) |
| `src/gui/*` (`xcb_wrapper.c`, `xcb_shm.c`, `window.c`, `event*.c`, `main_gui.c`) | Windowing, events, presentation | `silksurf-gui` (XCB backend + winit/softbuffer backend) | Rust owns; C deleted (AD-024 executed) |
| `src/neural/bpe.c`, `bpe_bench.c` | BPE tokenizer + benchmark (AD-006) | `silksurf-core` (`bpe::BpeTokenizer`, bench at `benches/bpe.rs`) | Rust owns; C deleted (AD-024 step 2 executed) |
| `src/ffi/js_engine_wrapper.c` | C-side bridge to `silksurf-js` | not needed (Rust callers use `silksurf_js::SilkContext`) | Deleted with the C binaries (AD-024 step 3 executed) |
| `src/main.c`, `src/webview.c` | C browser entry points | `silksurf-app` | Rust owns; C deleted (AD-024 executed) |
| `src/fuzz_harness.c`, `src/css/fuzz_css.c` | AFL fuzz harnesses | `fuzz/` (cargo-fuzz, 5 targets) | Rust owns; C harnesses and AFL seed trees deleted (AD-024 step 4 executed) |
| `tests/*.c` | CTest unit tests | `crates/*/tests`, `silksurf-js/tests` | Rust owns; C tests deleted with their modules |
| `src/core/` | (empty directory) | n/a | Deleted (AD-024 step 1 executed) |

## Removal record

AD-024 sequenced removal in four steps, each landed with the gate green:
dead code first (`tree_builder.c`, `src/core/`, the broken `gui`
target), then the BPE re-home into `silksurf_core::bpe`, then the FFI
shim with the C binaries, then the duplicated modules, `CMakeLists.txt`,
the C tests, and the AFL seed trees. Git history preserves every removed
source; this map is the durable pointer from each C module to its
owning crate.

## See

  * `docs/design/ARCHITECTURE-DECISIONS.md` -- AD-024 (retirement), AD-002
    (superseded C-side), AD-006 (BPE), AD-008 (stable-Rust migration)
  * `src/README_LEGACY.md` -- in-tree marker pointing here
  * `docs/roadmaps/DEBT-RECONCILIATION-ROADMAP.md` -- task breakdown
