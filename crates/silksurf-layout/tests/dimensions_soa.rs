/*
 * tests/dimensions_soa.rs -- Integration tests for DimensionsSoA.
 *
 * WHY: Verifies that the SoA round-trip is lossless (push then get
 * reconstructs identical Dimensions) and that the len invariant holds
 * (len() tracks the number of push() calls exactly).
 *
 * Gated on the dim-soa feature to avoid compile errors in the default build.
 */

#[cfg(feature = "dim-soa")]
mod tests {
    use silksurf_layout::dimensions_soa::DimensionsSoA;
    use silksurf_layout::{Dimensions, EdgeSizes, Rect};

    fn make_dims(base: f32) -> Dimensions {
        Dimensions {
            content: Rect {
                x: base,
                y: base + 1.0,
                width: base + 2.0,
                height: base + 3.0,
            },
            padding: EdgeSizes {
                top: base + 4.0,
                right: base + 5.0,
                bottom: base + 6.0,
                left: base + 7.0,
            },
            border: EdgeSizes {
                top: base + 8.0,
                right: base + 9.0,
                bottom: base + 10.0,
                left: base + 11.0,
            },
            margin: EdgeSizes {
                top: base + 12.0,
                right: base + 13.0,
                bottom: base + 14.0,
                left: base + 15.0,
            },
        }
    }

    /// round_trip -- push 3 distinct Dimensions and verify get() reconstructs
    /// each one exactly.
    ///
    /// WHY: The base values are small integers (0.0, 100.0, 200.0) that are
    /// exactly representable in f32, so bit-for-bit equality is guaranteed
    /// without any epsilon tolerance.
    #[test]
    fn round_trip() {
        let d0 = make_dims(0.0);
        let d1 = make_dims(100.0);
        let d2 = make_dims(200.0);

        let mut soa = DimensionsSoA::new();
        soa.push(&d0);
        soa.push(&d1);
        soa.push(&d2);

        assert_eq!(soa.get(0), Some(d0), "index 0 round-trip failed");
        assert_eq!(soa.get(1), Some(d1), "index 1 round-trip failed");
        assert_eq!(soa.get(2), Some(d2), "index 2 round-trip failed");
        assert_eq!(soa.get(3), None, "out-of-bounds get should return None");
    }

    /// len_invariant -- after N push() calls, len() must equal N.
    ///
    /// WHY: All 16 parallel Vecs must grow in lock-step; this test detects
    /// any push() implementation that accidentally skips one Vec.
    #[test]
    fn len_invariant() {
        let mut soa = DimensionsSoA::new();
        assert!(soa.is_empty(), "new container must be empty");

        let n = 10usize;
        for i in 0..n {
            assert_eq!(soa.len(), i, "len before push {i} should be {i}");
            soa.push(&make_dims(i as f32));
            assert_eq!(soa.len(), i + 1, "len after push {i} should be {}", i + 1);
        }
        assert!(
            !soa.is_empty(),
            "non-empty container must not report is_empty"
        );
        assert_eq!(soa.len(), n, "final len must equal push count");
    }
}
