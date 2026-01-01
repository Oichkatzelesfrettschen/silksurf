# Engine Hot Paths (DOM/CSS/Layout/Render)

This audit focuses on hot loops and allocation pressure outside the JS runtime.
File paths are included for fast navigation.

## DOM + HTML
- Tokenization loop: `crates/silksurf-html/src/lib.rs`
  - Delimiter-first scans with memchr; character reference decode fast path.
- Tree construction: `crates/silksurf-html/src/tree_builder.rs`
  - `process_tokens` and node creation in batches.
- DOM mutation: `crates/silksurf-dom/src/lib.rs`
  - `push_node`, `append_child`, `set_attribute` (id/class interning).
  - Mutation batching defers dirty-node lists until flush.

## CSS
- Tokenizer: `crates/silksurf-css/src/lib.rs`
  - `CssTokenizer::feed` and string parsing paths.
- Selector parsing: `crates/silksurf-css/src/selector.rs`
  - `parse_selector_list` and modifier parsing; allocates lists.
- Selector matching: `crates/silksurf-css/src/matching.rs`
  - `matches_selector` recursion and attribute lookups.
- Cascade: `crates/silksurf-css/src/style.rs`
  - `cascade_for_node` walks indexed candidates (tag/id/class buckets).
  - Selector specificity is cached in the index; rule-order iteration is linear.
  - StyleCache used in `silksurf-engine` to centralize computed styles.
  - Incremental recompute wired to DOM mutation batching.

## Layout
- Tree build: `crates/silksurf-layout/src/lib.rs`
  - `build_layout_tree` and `build_layout_box` recursion.
- Flow layout: `layout_block` inline loop and `inline_text_width` width calc.
  - Fixed-point conversions (`FIXED_SCALE`) can be hoisted to reduce churn.

## Render
- Display list build: `crates/silksurf-render/src/lib.rs`
  - `build_display_list_for_box` recursion; text stored as `NodeId` handles.
- Rasterization: `fill_rect` row fill loop; bandwidth bound.
  - Display list tiles support damage-region filtering via `rasterize_damage`.

## Allocation Hotspots
- Selector lists and modifiers (`Vec` growth per rule).
- Layout children vectors (now arena-backed).
- Display list item vectors (text clones still present; eliminate next).
- Style cache map churn on full recompute; incremental invalidation now wired.
