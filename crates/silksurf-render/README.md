# silksurf-render

Display-list construction and tile-parallel rasterization to a
framebuffer.

## Public API

  * `DisplayList`, `DisplayItem`, `Color`, `Rect` -- the
    intermediate-representation between layout and pixels.
  * `build_display_list(dom, styles, layout) -> DisplayList` -- emit
    items from a styled layout tree.
  * `rasterize_into(buf, width, height, list)` -- single-thread
    rasterizer.
  * `rasterize_parallel(width, height, list) -> Vec<u8>` -- tile-
    parallel rasterizer with rayon (owned-output).
  * `rasterize_parallel_into(buf, width, height, list)` -- tile-
    parallel rasterizer that REUSES a caller-owned buffer; preferred
    for interactive rendering.

## SIMD

`fill_row_sse2` (gated on `is_x86_feature_detected!("sse2")`) does
4-pixel SIMD writes via `_mm_storeu_si128`. NEON path TBD (roadmap
P8.S7). All `unsafe` blocks here are documented at
`docs/design/UNSAFE-CONTRACTS.md`.

## Tiles + rayon

  * Tile size: 64x64 pixels (chosen for L1 fit on x86_64).
  * `SendPtr` newtype wraps `*mut u8` with `unsafe impl Send +
    Sync` -- safety justification: rayon scope guarantees disjoint
    tile regions, no thread writes outside its tile (see UNSAFE-
    CONTRACTS).

## Hot-path notes

  * `rasterize_parallel_into` reuses the caller-provided `Vec<u8>` so
    the 4 MB framebuffer alloc happens once per dimension change, not
    per frame.
  * The Phase-4.4 DisplayList type-batched rasterization TODO at
    `lib.rs` is queued in roadmap P4; expected to improve branch
    prediction during fill.

## Status

Functional for solid-color rectangles. Image decode, gradient fill,
text rendering (font/shape), filters all pending.

## See Also

  * `docs/design/UNSAFE-CONTRACTS.md` -- per-block SAFETY index
  * `docs/PERFORMANCE.md` -- bench numbers including parallel raster
