/*
 * tests/determinism.rs -- SIMD vs scalar fill determinism verification.
 *
 * WHY: The render pipeline has two fill code paths -- fill_row_sse2 (SSE2
 * vector stores, 4 pixels/instruction) and a scalar fallback (slice::fill).
 * Both must produce bit-identical output for every (length, color) pair.
 * A bug in the SIMD tail-handling or byte-order logic would be silent if
 * only one path is exercised; this test catches that class of regression.
 *
 * WHAT: Calls silksurf_render::fill_scalar and silksurf_render::fill_simd
 * on freshly allocated buffers of identical size and asserts byte equality.
 * Covers three pixel counts (non-aligned, SIMD-aligned, large) and five
 * color values (transparent black, opaque white, opaque red, mid-alpha gray,
 * and a four-distinct-channel value to expose byte-order bugs).
 *
 * HOW: `cargo test -p silksurf-render` runs this file as a separate test
 * binary linked against the crate. No nightly features required.
 *
 * See: crates/silksurf-render/src/lib.rs fill_scalar / fill_simd
 */

use silksurf_render::{fill_scalar, fill_simd};

// ---------------------------------------------------------------------------
// Pixel counts: non-aligned (3 -- shorter than one SSE2 chunk), SIMD-aligned
// (16 -- exactly four 4-wide SSE2 stores), and large (4001 -- exercises the
// scalar tail after many SIMD stores and ensures loop termination is correct).
// ---------------------------------------------------------------------------
const COUNTS: &[usize] = &[3, 16, 4001];

// ---------------------------------------------------------------------------
// Color values chosen to stress distinct failure modes:
//   0x00000000 -- all-zero; a memset-to-zero bug would only be caught here.
//   0xFFFFFFFF -- all-ones; complement of the above.
//   0xFF0000FF -- opaque red in ABGR; exercises R-channel byte placement.
//   0x80808080 -- mid-alpha 50-percent-gray; catches alpha-channel truncation.
//   0x12345678 -- four distinct nibble values; byte-order bugs produce a
//                 permuted value that differs from the scalar path.
// ---------------------------------------------------------------------------
const COLORS: &[u32] = &[0x0000_0000, 0xFFFF_FFFF, 0xFF00_00FF, 0x8080_8080, 0x1234_5678];

fn make_buf(len: usize) -> Vec<u32> {
    vec![0xDEAD_BEEF_u32; len]
}

// ---------------------------------------------------------------------------
// Core assertion: fill_scalar and fill_simd must produce identical bytes for
// every (count, color) combination enumerated above.
// ---------------------------------------------------------------------------
fn assert_paths_identical(count: usize, color: u32) {
    let mut scalar_buf = make_buf(count);
    let mut simd_buf = make_buf(count);

    fill_scalar(&mut scalar_buf, color);
    fill_simd(&mut simd_buf, color);

    assert_eq!(
        scalar_buf, simd_buf,
        "fill_scalar and fill_simd diverge for count={count} color=0x{color:08X}"
    );

    // Verify both paths actually wrote the expected value to every element.
    // If the fill is a no-op both buffers would compare equal but wrong.
    for (i, &px) in scalar_buf.iter().enumerate() {
        assert_eq!(
            px, color,
            "scalar fill wrote wrong value at index {i}: expected 0x{color:08X} got 0x{px:08X}"
        );
    }
}

// ---------------------------------------------------------------------------
// Test: non-aligned count (3 pixels -- less than one 4-wide SSE2 store).
// The SIMD path must not overread or write garbage via the scalar tail.
// ---------------------------------------------------------------------------
#[test]
fn simd_scalar_identical_non_aligned() {
    for &color in COLORS {
        assert_paths_identical(3, color);
    }
}

// ---------------------------------------------------------------------------
// Test: SIMD-aligned count (16 pixels -- exactly four 4-wide SSE2 stores with
// no tail). Confirms the vector loop itself is correct.
// ---------------------------------------------------------------------------
#[test]
fn simd_scalar_identical_aligned() {
    for &color in COLORS {
        assert_paths_identical(16, color);
    }
}

// ---------------------------------------------------------------------------
// Test: large buffer (4001 pixels -- many SSE2 stores plus a 1-element tail).
// Catches off-by-one errors in the loop bound calculation.
// ---------------------------------------------------------------------------
#[test]
fn simd_scalar_identical_large() {
    for &color in COLORS {
        assert_paths_identical(4001, color);
    }
}

// ---------------------------------------------------------------------------
// Test: zero-length buffer edge case. Neither path should panic.
// ---------------------------------------------------------------------------
#[test]
fn simd_scalar_identical_empty() {
    for &color in COLORS {
        let mut scalar_buf: Vec<u32> = Vec::new();
        let mut simd_buf: Vec<u32> = Vec::new();
        fill_scalar(&mut scalar_buf, color);
        fill_simd(&mut simd_buf, color);
        assert_eq!(
            scalar_buf, simd_buf,
            "empty buffer mismatch for color=0x{color:08X}"
        );
    }
}

// ---------------------------------------------------------------------------
// Test: single-element buffer (1 pixel). Falls entirely into the scalar tail
// of the SIMD path; validates that no vector store is attempted.
// ---------------------------------------------------------------------------
#[test]
fn simd_scalar_identical_single_pixel() {
    for &color in COLORS {
        assert_paths_identical(1, color);
    }
}

// ---------------------------------------------------------------------------
// Test: exhaustive cross-product over all (count, color) pairs defined above.
// Acts as a regression catch-all in case individual tests are skipped.
// ---------------------------------------------------------------------------
#[test]
fn simd_scalar_cross_product() {
    for &count in COUNTS {
        for &color in COLORS {
            assert_paths_identical(count, color);
        }
    }
}
