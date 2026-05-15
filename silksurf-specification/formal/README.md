# Formal Specifications -- silksurf-specification/formal/

This directory contains TLA+ formal models for two core silksurf subsystems.
Both specs are written for TLA+ 2 and are checkable with TLC.

TLC is NOT part of the local build gate (cmake/ctest).  Run it manually as
described below.  It requires a separate JVM-based install; see
https://github.com/tlaplus/tlaplus/releases for the current distribution.


## resolve_table.tla

**What it models**: The monotonic CSS resolve table in silksurf-core.
The table is a lock-free partial map from selector generation numbers to
computed style indices.  Once a generation number is bound, its binding is
permanent -- no re-binding, no invalidation.

**Operations modelled**:
- Insert(gen, idx) -- bind a generation to a style index (CAS-protected in Rust)
- Lookup(gen)      -- pure read; no state change
- Clear            -- permanently disabled (FALSE guard); documents that eviction
                      is explicitly prohibited by the design contract

**Key invariant**: Monotonicity -- every generation in the table retains its
original binding across all reachable states.

**How to run**:

    tlc resolve_table.tla -config resolve_table.cfg

The supplied resolve_table.cfg uses GenNums = {0,1,2,3} and StyleIdxs = {0..7},
which gives a small but representative state space TLC exhausts in seconds.
Widen GenNums to explore more combinations (runtime grows combinatorially).


## cache_coherence.tla

**What it models**: The HTTP response cache in silksurf-net (cache.rs).
The cache maps URL keys to HTTP response values with a bounded capacity.
Disk persistence (the with_disk path) is intentionally out of scope; this
spec covers the in-memory coherence contract only.

**Operations modelled**:
- Store(k, v) -- insert or overwrite a cache entry (models ResponseCache::put)
- Lookup(k)   -- pure read; disabled when key is absent (models ResponseCache::get)
- Evict(k)    -- remove one entry (models single-key eviction; ResponseCache::clear
                 is Evict applied to every key)

**Key invariant**: Coherence -- every key present in the cache maps to a valid
value.  The sequential coherence theorem (Lookup after Store with no intervening
Evict returns the stored value) is stated as a THEOREM comment in the spec and
follows directly from the assignment in Store.

**How to run**:

    tlc cache_coherence.tla -config cache_coherence.cfg

The supplied cache_coherence.cfg uses 3 keys, 2 values, and MAX_CAPACITY = 3.
TLC exhausts this space quickly.  Add more keys or values for deeper coverage.


## Notes

- Both .cfg files list the invariants to check.  If TLC reports a violation,
  the counterexample trace pinpoints which state sequence breaks the property.
- TLC is NOT invoked by cmake, ctest, or CI.  It is a manual verification step.
- The specs follow the style of BrowserLoader.tla at the repository root.
- Unicode is not used; all text is ASCII for portability across editors and CI logs.
