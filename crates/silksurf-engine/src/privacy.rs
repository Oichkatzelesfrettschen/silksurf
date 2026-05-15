/*
 * Privacy and site isolation skeleton.
 *
 * WHY: AD-022 establishes the API surface for cookie jar partitioning,
 * third-party storage partitioning, and the partition key function.  The
 * implementation is explicitly deferred; these stubs mark where the real
 * work belongs so future contributors find a clear hook rather than a
 * blank file.
 *
 * WHAT: Three public items:
 *   - CookieJar        -- owns all cookies for one browsing context.
 *   - StoragePartition -- owns the partitioned storage key for a context.
 *   - partition_key()  -- placeholder that maps an origin to its storage
 *                         partition key; returns the origin unchanged until
 *                         the (site, top-level-site) scheme is implemented.
 *
 * HOW: Wire `pub mod privacy;` into `crates/silksurf-engine/src/lib.rs`.
 * When P9 networking maturity lands, replace the CookieJar body with a
 * field holding a parsed cookie store (e.g. the `cookie_store` crate or a
 * bespoke implementation).  When storage partitioning lands (P10), replace
 * the `partition_key` body with a function that hashes (site, top-level-
 * site) into a stable key string.
 *
 * See: AD-022 in docs/design/ARCHITECTURE-DECISIONS.md
 */

/// Placeholder cookie jar.
///
/// TODO (AD-022, P9+): Add a parsed cookie store field.  The jar must
/// enforce SameSite semantics, Secure/HttpOnly attributes, and cookie
/// jar partitioning keyed on (site, top-level-site) tuples.  Until then
/// this struct is a reservation: it compiles, it is exported, and callers
/// can hold a value of this type without changes when the fields land.
#[derive(Debug, Default)]
pub struct CookieJar {
    // TODO (AD-022): replace with a concrete cookie store once the HTTP
    // Set-Cookie parser and session model are in place (P9).
}

/// Placeholder storage partition descriptor.
///
/// TODO (AD-022, P10+): Add the partition key field (a (site, top-level-
/// site) tuple or a hash of it).  This struct will gate access to
/// localStorage, IndexedDB, and Cache Storage so that a third-party origin
/// embedded on site A cannot observe data it wrote when embedded on site B.
#[derive(Debug, Default)]
pub struct StoragePartition {
    // TODO (AD-022): add a `key: String` field derived from partition_key()
    // once the storage layer exists (P10).
}

/// Returns the partition key for the given origin.
///
/// PLACEHOLDER: Currently returns the origin unchanged.  When (site,
/// top-level-site) partitioning is implemented (AD-022, P10), this
/// function will compute a canonical key of the form
/// `"<registrable-site>/<top-level-registrable-site>"` and callers will
/// automatically pick up the new semantics.
///
/// `origin` must be a serialised origin as defined by the HTML spec
/// (scheme + "://" + host + optional port).  An opaque origin ("null")
/// is returned as-is; it is already maximally partitioned.
pub fn partition_key(origin: &str) -> String {
    // PLACEHOLDER: return origin verbatim until (site, top-level-site)
    // partitioning is implemented.  See AD-022 for the full design.
    origin.to_owned()
}
