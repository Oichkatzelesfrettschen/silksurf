# TODO/FIXME Audit

Heuristic scan for TODO/FIXME markers in first-party files.
Generated: 2025-12-31 23:20 UTC

## CLAUDE.md
- L16: -   **BUILD OUT**: Implement full solutions. No "TODO implement later" for core logic. Partial fixes accumulate debt.

## PHASE-2-COMPLETION-SUMMARY.md
- L38: - ✅ NO SHORTCUTS: No placeholders, no "TODO implement", full working examples

## diff-analysis/JS-ENGINE-ARCHITECTURE.md
- L345: // TODO: Hook to SilkSurf alert dialog
- L459: - ✅ Simple web app runs (TODO list, counter, etc.)

## diff-analysis/NEURAL-SILKSURF-ROADMAP.md
- L696: ## VI. REFINED TODO LIST (Granular, Actionable)

## diff-analysis/file-types.txt
- L325: 1 neosurf-fork/contrib/libdom/src/html/TODO

## diff-analysis/neosurf-files.txt
- L538: neosurf-fork/contrib/libdom/src/html/TODO

## diff-analysis/neosurf-relative.txt
- L538: contrib/libdom/src/html/TODO

## diff-analysis/only-in-neosurf.txt
- L538: contrib/libdom/src/html/TODO

## docs/ENGINE_PERF_ROADMAP.md
- L13: - TODO: Pre-index rules by tag/id/class to avoid full rule scans.
- L14: - TODO: Cache selector specificity and pre-sorted rule lists.
- L15: - TODO: Add per-node style cache keyed by NodeId + style generation.
- L20: - TODO: Expand fixed-point usage for margins/padding and box metrics.
- L23: - TODO: Tile display list by damage region.
- L24: - TODO: SIMD row fill for solid colors (widened stores).
- L25: - TODO: Avoid text clone in display list; store `SmallString` or slices.
- L28: - TODO: End-to-end `bench_pipeline` that reports parse/layout/render timing.
- L29: - TODO: Micro-bench for selector matching + cascade cost.
- L30: - TODO: Add perf CI guardrails (time + alloc thresholds).

## docs/IMPLEMENTATION_ROADMAP.md
- L786: 1. Update TODO list at start of day
- L790: 5. Update TODO list at end of day

## docs/LOGGING.md
- L17: ## TODO

## docs/PHASE4_DESIGN.md
- L321: /* TODO: Font rasterization via FreeType or bitmap fonts

## silksurf-js/src/bin/main.rs
- L49: // TODO: Execute code
- L52: // TODO: Execute file
- L55: // TODO: Start REPL

## silksurf-js/src/bytecode/compiler.rs
- L741: // TODO: Handle spread
- L783: // TODO: Handle spread
- L821: // TODO: Handle spread

## silksurf-js/src/ffi.rs
- L257: // TODO: Wire up actual heap stats when GC tracking is exposed

## silksurf-js/src/lexer/lexer.rs
- L572: // TODO: Handle ${...} expressions properly

## silksurf-js/src/parser/parser.rs
- L78: source_type: SourceType::Script, // TODO: detect module
- L594: is_async: false, // TODO: handle async

## silksurf-js/tests/test262/harness.rs
- L351: // TODO: Full parse and execute

## src/css/selector.c
- L17: /* TODO: Implement class list check after attribute interning */

## src/document/css_engine.c
- L25: /* TODO: Add hash table for style caching */
- L34: /* TODO: Implement URL resolution for @import and url() */
- L44: /* TODO: Implement stylesheet importing */
- L53: /* TODO: Implement font resolution */
- L445: /* TODO: Traverse DOM to find all <style> elements */
- L452: fprintf(stderr, "[CSS] TODO: Document style application not yet implemented\n");

## src/document/css_select_handler.c
- L57: /* TODO: Implement class attribute parsing */
- L71: /* TODO: Implement ID attribute retrieval */
- L85: /* TODO: Implement attribute checking */
- L98: /* TODO: Implement attribute value retrieval */
- L111: /* TODO: Implement class checking */
- L124: /* TODO: Implement ID checking */
- L276: /* TODO: Implement sibling counting for :nth-child */
- L474: /* TODO: Implement by comparing node name with qname */

## src/document/document.c
- L256: /* TODO: Implement layout algorithm
- L272: /* TODO: Implement rendering
- L302: /* TODO: Implement ID lookup
- L320: /* TODO: Implement event handling
- L334: /* TODO: Implement JavaScript execution (Phase 4e)
- L391: /* TODO: Track rendering state during render pass */
- L410: /* TODO: Queue damage region for scroll changes */

## src/document/html_tokenizer.c
- L694: /* TODO: Full UTF-8 encoding for non-ASCII characters */
- L1483: /* TODO: Only if in foreign content. For now, assume not. */
- L1896: /* TODO: Handle PUBLIC/SYSTEM keywords */
- L2052: /* TODO: Implement full 12.2.6.1 Character reference overrides table (0x80-0x9F) */
- L2822: /* TODO: Handle ambiguous ampersand */
- L2910: /* TODO: Implement remaining states */

## src/document/tree_builder.c
- L227: /* TODO: If deep=true, recursively clone children */
- L291: /* TODO: Implement form association if needed */
- L333: /* TODO: Track document quirks mode if needed */
- L343: /* TODO: Handle encoding change if needed */
- L352: /* TODO: Handle script completion */

## src/gui/window.c
- L84: /* TODO: Set window title via silk_display_t */
- L140: /* TODO: Implement XShm or pixmap-based image transfer

## src/gui/xcb_wrapper.c
- L253: /* TODO: Implement proper image transfer with XShm optimization */

## src/rendering/paint.c
- L32: /* TODO: Actually parse style attributes or computed style */
- L64: /* TODO: Real layout engine will compute these */

## src/rendering/pixel_ops.c
- L18: /* TODO: Implement CPUID-based detection for SSE2/AVX2

## tests/test_html5lib_harness.c
- L433: /* TODO: Verify attributes and self-closing flag */
