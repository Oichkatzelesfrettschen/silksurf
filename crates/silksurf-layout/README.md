# silksurf-layout

Box-model + flexbox layout. Walks the styled DOM tree and produces
positioned `Rect`s consumed by `silksurf-render`.

## Public API

  * `LayoutTree`, `LayoutBox`, `BoxType`, `Dimensions`, `Edges`, `Rect`.
  * `build_layout_tree`, `build_layout_tree_incremental` -- top-level
    entry points.
  * `LayoutNeighborTable` -- pre-computed BFS-level decomposition for
    parallel layout. Owns `level_starts: Vec<u32>` (flat offsets,
    no inner-Vec allocations), `parent_idx: Vec<u32>`, `bfs_order:
    Vec<NodeId>`, `child_count: Vec<u16>`, `node_to_bfs_idx`. See
    GLOSSARY -> generation-gated rebuild.

## Layout passes

  1. Build the layout tree (block / inline / anonymous boxes).
  2. Resolve dimensions in BFS order, with the `LayoutNeighborTable`
     amortising parent / sibling / child lookups to O(1).
  3. Fused pipeline path skips the intermediate `LayoutTree` and emits
     `Rect`s directly into the workspace (see
     `silksurf-engine::fused_pipeline`).

## Hot-path notes

  * `LayoutNeighborTable::rebuild(dom)` is in-place; reuses capacity
    across calls. Replaces the prior `Vec<Vec<NodeId>>` `levels` field
    that allocated O(depth) inner Vecs per call.
  * The Phase-4.4 `Dimensions` SoA TODO at `lib.rs` is queued in
    roadmap P4; expected to further reduce per-node fetch cost.

## Status

Functional for block + inline + flex box-model basics. Position
absolute/relative/fixed and CSS Grid are pending; tracked in roadmap.
