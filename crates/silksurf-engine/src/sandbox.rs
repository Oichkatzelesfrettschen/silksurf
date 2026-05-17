//! Privacy / sandboxing skeleton.
//!
//! WHY: a v0.1 single-origin browser engine has no site isolation,
//! third-party cookie policy, or storage partitioning. The threat model
//! (`docs/design/THREAT-MODEL.md`, "Cross-cutting -- not yet started")
//! catalogues the gap. This module is the API skeleton where the
//! enforcement points will land.
//!
//! WHAT (planned):
//!
//!   * `OriginContext` -- a per-origin handle that owns the cookie jar
//!     for that origin, the storage partition key, and any
//!     fingerprinting-budget state.
//!   * `SiteIsolation` -- a registry that maps origins to their
//!     `OriginContext` and enforces that JS / fetch / storage requests
//!     from origin A never observe origin B's state.
//!   * `StoragePartition` -- the on-disk segregation that backs the
//!     persistent response cache and any localStorage. Today the
//!     persistent cache (`silksurf-net::cache`) is unpartitioned;
//!     P8.S9 partitions it by top-level origin.
//!
//! HOW (planned): the engine will accept an `Option<OriginContext>` on
//! every fetch / cache / DOM-evaluation call. None = the legacy
//! "single-origin" v0.1 mode (today's behaviour). Some(ctx) = enforced
//! site isolation. The transition is a feature flag on
//! `silksurf-app`.
//!
//! Tracked in the SNAZZY-WAFFLE roadmap P8.S9. ADR-021 (deferred)
//! will record the cross-cutting decision once the design firms up.

use std::sync::Arc;

/// Per-origin context. Empty in v0.1; populated when P8.S9 lands.
#[derive(Debug, Default, Clone)]
pub struct OriginContext {
    /// Top-level origin (scheme + host + port). Empty placeholder.
    pub origin: Arc<str>,
}

impl OriginContext {
    pub fn new(origin: impl Into<Arc<str>>) -> Self {
        Self {
            origin: origin.into(),
        }
    }
}

/// Site-isolation registry -- placeholder.
#[derive(Debug, Default)]
pub struct SiteIsolation {
    contexts: Vec<OriginContext>,
}

impl SiteIsolation {
    #[must_use] 
    pub fn new() -> Self {
        Self::default()
    }

    /// Lookup or create an `OriginContext` for the given origin string.
    /// In v0.1 every call gets a fresh context; real isolation lands in
    /// P8.S9.
    pub fn context_for(&mut self, origin: &str) -> OriginContext {
        if let Some(existing) = self.contexts.iter().find(|c| c.origin.as_ref() == origin) {
            existing.clone()
        } else {
            let new_ctx = OriginContext::new(origin);
            self.contexts.push(new_ctx.clone());
            new_ctx
        }
    }
}

/// Storage partition key. In v0.1 every request shares the same
/// partition; P8.S9 keys by top-level origin so two tabs of two sites
/// see different storage views.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct StoragePartition {
    pub key: Arc<str>,
}

impl StoragePartition {
    pub const SHARED: &'static str = "<v0.1-shared-partition>";

    #[must_use] 
    pub fn shared() -> Self {
        Self {
            key: Arc::from(Self::SHARED),
        }
    }

    #[must_use] 
    pub fn for_origin(origin: &str) -> Self {
        Self {
            key: Arc::from(origin),
        }
    }
}
