//! Forensics-grade primitives for reproducible tests.
//!
//! WHY: Wall-clock time and OS PRNG state make scheduling and randomized
//! tests bit-identical only by accident. The two types in this module --
//! [`Clock`] and [`Rng`] -- give the test author full control over both
//! axes so failures reproduce on any host, on any day, in any order.
//!
//! Both types are intentionally minimal: no allocator, no threads, no
//! unsafe. The PRNG is `xorshift64` with a fixed seed -- adequate for
//! deterministic test fixtures and explicitly NOT cryptographic.
//!
//! See: SNAZZY-WAFFLE roadmap P8.S10 (forensics + repro primitives).

/// Deterministic clock for reproducible test timing.
///
/// WHY: Wall-clock time in tests produces flaky ordering; a virtual
/// clock makes scheduling tests bit-identical across runs. Tests can
/// freeze time, advance it by exact deltas, and assert on the resulting
/// timestamps without dealing with monotonic-clock skew or thread
/// preemption noise.
///
/// The internal counter is u64 nanoseconds, which spans ~584 years
/// from any chosen origin -- comfortably wider than any test horizon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clock {
    now_nanos: u64,
}

impl Clock {
    /// Create a clock at `start_nanos` since an arbitrary epoch.
    pub fn new(start_nanos: u64) -> Self {
        Self {
            now_nanos: start_nanos,
        }
    }

    /// Read the current virtual time, in nanoseconds.
    pub fn now_nanos(&self) -> u64 {
        self.now_nanos
    }

    /// Advance the virtual clock by `delta_nanos`.
    ///
    /// Saturates on overflow so a runaway test asserting on monotonic
    /// progression cannot accidentally panic the harness.
    pub fn advance(&mut self, delta_nanos: u64) {
        self.now_nanos = self.now_nanos.saturating_add(delta_nanos);
    }
}

/// Seedable PRNG for reproducible test randomness.
///
/// WHY: Fuzz reductions, randomized property tests, and shuffle-based
/// schedulers all need a PRNG whose stream depends only on a recorded
/// seed. We use `xorshift64` (Marsaglia 2003) -- one 64-bit add of
/// state, three xors, one shift each. ~1 ns per call on x86_64. Period
/// is 2^64 - 1, which is plenty for test fixtures.
///
/// NOT cryptographically secure. Do NOT use for token generation, key
/// material, or anything else where predictability is a security flaw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// Create a PRNG from a fixed seed.
    ///
    /// The seed must be non-zero. A zero seed would lock xorshift64
    /// to the all-zero stream forever, so we substitute a fixed sentinel
    /// (the golden-ratio constant 0x9E3779B97F4A7C15) instead of
    /// silently producing garbage.
    pub fn new(seed: u64) -> Self {
        let state = if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        };
        Self { state }
    }

    /// Produce the next 64-bit pseudo-random value.
    ///
    /// Algorithm: xorshift64 (G. Marsaglia, "Xorshift RNGs", 2003).
    /// Mutates `self.state` in place; same seed = same sequence.
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    /// Produce a uniform double in the half-open interval [0, 1).
    ///
    /// Implementation: take the high 53 bits of next_u64() (the mantissa
    /// width of an IEEE-754 double) and divide by 2^53. This is the
    /// canonical conversion that avoids the off-by-one bias of
    /// `(u64 as f64) / u64::MAX as f64`.
    pub fn next_f64(&mut self) -> f64 {
        let bits = self.next_u64() >> 11; // keep 53 bits
        // 2^53 as f64 is exact.
        bits as f64 / ((1u64 << 53) as f64)
    }

    /// Produce a uniform integer in the half-open interval [lo, hi).
    ///
    /// Returns `lo` if `hi <= lo` so the caller never has to special-case
    /// empty ranges. The mapping is the simple `lo + (rng % span)`
    /// reduction; this is acceptable for test fixtures (the modulo bias
    /// is below 1 ULP for spans much smaller than 2^64). A
    /// rejection-sampling variant would be needed for cryptographic use,
    /// which this PRNG explicitly does not target.
    pub fn next_range(&mut self, lo: u64, hi: u64) -> u64 {
        if hi <= lo {
            return lo;
        }
        let span = hi - lo;
        lo + (self.next_u64() % span)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_advances_by_exact_delta() {
        let mut clock = Clock::new(1_000);
        assert_eq!(clock.now_nanos(), 1_000);
        clock.advance(250);
        assert_eq!(clock.now_nanos(), 1_250);
        clock.advance(0);
        assert_eq!(clock.now_nanos(), 1_250);
        clock.advance(u64::MAX); // saturating
        assert_eq!(clock.now_nanos(), u64::MAX);
    }

    #[test]
    fn rng_same_seed_same_sequence() {
        let mut rng_a = Rng::new(0xDEAD_BEEF_CAFE_F00D);
        let mut rng_b = Rng::new(0xDEAD_BEEF_CAFE_F00D);
        for _ in 0..256 {
            assert_eq!(rng_a.next_u64(), rng_b.next_u64());
        }

        // Different seeds diverge.
        let mut rng_c = Rng::new(0xDEAD_BEEF_CAFE_F00E);
        let first_a = Rng::new(0xDEAD_BEEF_CAFE_F00D).next_u64();
        let first_c = rng_c.next_u64();
        assert_ne!(first_a, first_c);

        // Zero seed is remapped, not stuck on zero forever.
        let mut rng_zero = Rng::new(0);
        assert_ne!(rng_zero.next_u64(), 0);
    }

    #[test]
    fn rng_next_range_stays_within_bounds() {
        let mut rng = Rng::new(0x0123_4567_89AB_CDEF);
        for _ in 0..10_000 {
            let value = rng.next_range(100, 200);
            assert!(value >= 100, "value {value} below lo");
            assert!(value < 200, "value {value} at or above hi");
        }
        // Empty range degenerates to lo.
        let mut rng2 = Rng::new(1);
        assert_eq!(rng2.next_range(50, 50), 50);
        assert_eq!(rng2.next_range(50, 10), 50);

        // f64 sample is always in [0, 1).
        let mut rng3 = Rng::new(42);
        for _ in 0..1_000 {
            let value = rng3.next_f64();
            assert!(value >= 0.0, "f64 {value} negative");
            assert!(value < 1.0, "f64 {value} not below 1.0");
        }
    }
}
