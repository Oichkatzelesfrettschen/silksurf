# silksurf-render OPERATIONS

## Runtime tunables

No environment variables are consumed at runtime. SIMD path selection is automatic at startup via feature detection.

## SIMD dispatch

`fill_row_u32` selects the fastest available path at runtime:

| Architecture | Path | Detection |
|---|---|---|
| x86 / x86_64 | `fill_row_sse2` (4 pixels/store via `_mm_storeu_si128`) | `is_x86_feature_detected!("sse2")` |
| AArch64 | `fill_row_neon` (4 pixels/store via `vst1q_u32`) | `is_aarch64_feature_detected!("neon")` |
| Other | scalar `.fill()` | always |

On aarch64-unknown-linux-gnu NEON is mandatory, so the NEON path is always taken. On x86_64 SSE2 has been baseline since 2003; the scalar fallback is unreachable in practice.

## Common failure modes

### Blank output (all-zero pixels)

Cause: `rasterize_damage` clamps to the damage rect; if the damage rect is empty or misaligned with the tile grid, no tiles are painted.

Fix: verify `damage` argument covers at least one tile (64x64 pixels). Call `DisplayList.with_tiles(width, height, 64)` before rasterizing. Check that `DisplayList.items` is non-empty.

### Pixel buffer wrong size

Cause: `rasterize_parallel` returns a `Vec<u8>` sized `width * height * 4`; confusion between `Vec<u8>` and `Vec<u32>` can cause the caller to interpret misaligned data.

Fix: use `rasterize_parallel_into` with a pre-allocated `Vec<u8>` that matches `width * height * 4`.

### SIMD vs scalar divergence

Both `fill_simd` and `fill_scalar` are exported for testing. The determinism test (`tests/determinism.rs`) verifies they produce identical output for all input combinations. If you suspect divergence, run `cargo test -p silksurf-render --test determinism`.

### Rayon tile panic

`rasterize_parallel` uses a `SendPtr` wrapper for parallel tile rasterization. If threads access overlapping tiles (should not happen given the tile partitioning), a data race occurs at the pointer level. The implementation uses `debug_assert!` to verify disjoint regions -- run debug builds to surface this.

## Key constants

| Constant | Value | Description |
|---|---|---|
| Tile size | 64 pixels | Used by `with_tiles`; hardcoded in the hot path |
| ARGB pixel format | 0xAARRGGBB | Byte order for `Vec<u32>` buffer |
| sRGB gamma | IEC 61966-2-1 | Used in `srgb_to_linear` / `linear_to_srgb` |

## Color science

All compositing happens in linear light. Use `srgb_to_linear` before blending and `linear_to_srgb` when writing back to the pixel buffer. Alpha premultiplication is required before compositing (`premultiply`) and must be reversed afterwards (`unpremultiply`). See `docs/design/COLOR.md`.
