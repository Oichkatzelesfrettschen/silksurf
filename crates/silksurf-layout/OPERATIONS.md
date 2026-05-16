# silksurf-layout OPERATIONS

## Runtime tunables

No environment variables are consumed at runtime.

## Key invariants

- `build_layout_tree` takes a `&Dom` and an optional pre-computed style map. Callers must ensure `dom.generation()` matches the style map's generation if one is supplied -- passing a stale style map produces incorrect geometry.
- `LayoutNeighborTable::rebuild` is O(N) in DOM nodes. It must be called before each layout pass when the DOM has changed (gated by generation counter in `FusedWorkspace`).
- `Rect` coordinates are floating-point pixels in the same coordinate space as the viewport. Overflow is not clamped at this layer; the rasterizer clips to the buffer bounds.
- `DimensionsSoA` is an optional SoA cache for callers that need per-node geometry in column-oriented form. It is not maintained automatically; callers fill it via `DimensionsSoA::from_layout_tree` after each layout pass.

## Common failure modes

### All nodes have zero height

Cause: viewport height is zero or the document root has `display: none`.

Fix: verify the viewport `Rect` passed to `build_layout_tree` has `height > 0`. Check `ComputedStyle::display` for the root node.

### Text nodes produce incorrect width

Cause: font metrics are not wired (no HarfBuzz shaping). Text width is approximated as `char_count * font_size * 0.6`.

Fix: this is a known limitation (no HarfBuzz integration yet). For layout debugging, use a fixed-width font assumption or inspect the `TextNode` rect directly.

### `LayoutNeighborTable` panics on `unwrap` in `parent_of`

Cause: `parent_of` called with a `NodeId` that was not present when `rebuild` ran. Can happen if the DOM was mutated between `rebuild` and `parent_of`.

Fix: call `rebuild` after every DOM mutation batch. The `FusedWorkspace` generation check automates this.

## DoS bounds

| Bound | Enforced by |
|---|---|
| Layout tree depth | DOM tree depth (bounded by HTML parser `MAX_TOKENS_PER_FEED`) |
| `DimensionsSoA` size | Bounded by DOM node count |

No explicit cap on layout node count at this layer; rely on parser-level bounds.
