//! Storage partitioning: partition keys and the partitioned cookie store.
//!
//! `partition_key` derives the double-keyed storage partition string from the
//! top-level document's site and a resource's origin -- the mechanism that
//! stops a third-party embedded on site A from reading data it wrote when
//! embedded on site B. `StoragePartition` carries that key. Under the `net`
//! feature this module re-exports the cookie store from `silksurf-net` so the
//! cookie jar and partitioning live in one place.
//!
//! Scope (AD-022, amended): partition-key derivation and the partitioned
//! cookie store are implemented and tested, and the partitioned jar is wired
//! into the live HTTP request/response path (Set-Cookie parse on responses,
//! Cookie header on requests, one jar shared by fetch and the JS
//! `document.cookie` bridge). The site half of the key is the registrable
//! domain (eTLD+1): both `sandbox::Origin::site` and
//! `silksurf_net::cookie::site_of_url` derive it through `silksurf_core::psl`.
//! What is still deferred: process-level isolation (separate renderer
//! processes), and partitioning of storage beyond cookies (localStorage /
//! IndexedDB have no substrate yet).

use crate::sandbox::Origin;

#[cfg(feature = "net")]
pub use silksurf_net::cookie::{
    Cookie, CookieStore, PartitionedCookieStore, SameSite, SameSiteContext, site_of_url,
    subresource_same_site_context,
};

/// The partitioned storage key for a resource loaded under a top-level
/// document, derived from serialized origins. Both origins are reduced to their
/// site (scheme + registrable domain, via `silksurf_core::psl`); the key is
/// `"<resource-site>^<top-level-site>"`.
///
/// This is the origin-based derivation for storage-partition descriptors. The
/// canonical LIVE cookie keyer is `silksurf_net::cookie::partition_key`, which
/// takes already-computed sites and adds the empty-site -> unpartitioned
/// degradation for unplumbed fetch paths. Both produce the same
/// `"<resource-site>^<top-level-site>"` format for non-empty sites.
///
/// `top_level_origin` and `resource_origin` are serialized origins
/// (`scheme://host[:port]`). Opaque origins serialize their site as `null`,
/// which is already maximally partitioned.
#[must_use]
pub fn partition_key(top_level_origin: &str, resource_origin: &str) -> String {
    let top = Origin::parse(top_level_origin).site();
    let resource = Origin::parse(resource_origin).site();
    format!("{resource}^{top}")
}

/// A storage partition descriptor. Gates access to partitioned storage
/// (cookies today; localStorage/IndexedDB/Cache Storage when they land) so a
/// third-party origin cannot observe across top-level sites.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct StoragePartition {
    pub key: String,
}

impl StoragePartition {
    /// Build a partition for a resource under a top-level document.
    #[must_use]
    pub fn for_context(top_level_origin: &str, resource_origin: &str) -> Self {
        Self {
            key: partition_key(top_level_origin, resource_origin),
        }
    }

    /// Build a partition from a pre-computed key.
    #[must_use]
    pub fn from_key(key: impl Into<String>) -> Self {
        Self { key: key.into() }
    }

    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partition_key_double_keys_resource_and_top_level_site() {
        // Same resource origin under two different top-level sites -> two keys.
        let under_a = partition_key("https://a.test", "https://tracker.example");
        let under_b = partition_key("https://b.test", "https://tracker.example");
        assert_ne!(under_a, under_b);
        assert_eq!(under_a, "https://tracker.example^https://a.test");

        // Same context -> same key (port-independent site).
        let key1 = partition_key("https://a.test:443", "https://tracker.example");
        let key2 = partition_key("https://a.test", "https://tracker.example:8443");
        assert_eq!(key1, key2);
    }

    #[test]
    fn storage_partition_wraps_the_key() {
        let partition = StoragePartition::for_context("https://top.test", "https://res.test");
        assert_eq!(partition.key(), "https://res.test^https://top.test");
        assert_eq!(
            partition,
            StoragePartition::from_key("https://res.test^https://top.test")
        );
    }

    #[test]
    fn opaque_origins_partition_as_null() {
        let key = partition_key("null", "about:blank");
        assert_eq!(key, "null^null");
    }

    #[cfg(feature = "net")]
    #[test]
    fn partitioned_cookie_store_isolates_by_derived_key() {
        let mut store = PartitionedCookieStore::new();
        let key_a = partition_key("https://a.test", "https://shop.example");
        let key_b = partition_key("https://b.test", "https://shop.example");
        store
            .store_mut(&key_a)
            .set_from_set_cookie("cart=A", "shop.example", 0);
        store
            .store_mut(&key_b)
            .set_from_set_cookie("cart=B", "shop.example", 0);
        assert_eq!(
            store.store(&key_a).unwrap().document_cookie_string("", 0),
            "cart=A"
        );
        assert_eq!(
            store.store(&key_b).unwrap().document_cookie_string("", 0),
            "cart=B"
        );
    }
}
