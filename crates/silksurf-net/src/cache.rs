/*
 * cache.rs -- HTTP response cache with ETag/Last-Modified support.
 *
 * WHY: Sub-millisecond re-renders require zero network latency. This cache
 * stores the most recent response per URL. On re-navigation:
 *   1. Return cached response immediately (0ms)
 *   2. Start conditional fetch in background (If-None-Match / If-Modified-Since)
 *   3. If 304 Not Modified: no work needed
 *   4. If 200: compute delta and patch the rendered frame
 *
 * Inspired by the LBM perturbation formulation: store the "equilibrium"
 * (cached response) and only process "perturbations" (deltas from cache).
 * See: gororoba_app/docs/engine_optimizations.md Section 4
 *
 * Memory: responses stored in FxHashMap<String, CachedResponse>.
 * For ChatGPT: ~1.5MB (HTML + CSS). Acceptable for a browser cache.
 *
 * See: BasicClient::fetch() in lib.rs for the actual HTTP client
 * See: silksurf-app/src/main.rs for speculative pre-render integration
 */

use rustc_hash::FxHashMap;
use std::time::Instant;

/// A cached HTTP response with validation headers.
#[derive(Debug, Clone)]
pub struct CachedResponse {
    /// The full response body
    pub body: Vec<u8>,
    /// HTTP status code
    pub status: u16,
    /// Response headers (for content-type, etc.)
    pub headers: Vec<(String, String)>,
    /// ETag for conditional revalidation
    pub etag: Option<String>,
    /// Last-Modified for conditional revalidation
    pub last_modified: Option<String>,
    /// When this entry was cached
    pub cached_at: Instant,
    /// URL that produced this response
    pub url: String,
}

/// Simple in-memory HTTP response cache.
#[derive(Default)]
pub struct ResponseCache {
    entries: FxHashMap<String, CachedResponse>,
}

impl ResponseCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get a cached response for the given URL, if available.
    pub fn get(&self, url: &str) -> Option<&CachedResponse> {
        self.entries.get(url)
    }

    /// Store a response in the cache.
    pub fn put(&mut self, url: String, response: &super::HttpResponse) {
        let etag = response.header("etag").map(|s| s.to_string());
        let last_modified = response.header("last-modified").map(|s| s.to_string());

        self.entries.insert(
            url.clone(),
            CachedResponse {
                body: response.body.clone(),
                status: response.status,
                headers: response.headers.clone(),
                etag,
                last_modified,
                cached_at: Instant::now(),
                url,
            },
        );
    }

    /// Build conditional request headers for revalidation.
    pub fn conditional_headers(&self, url: &str) -> Vec<(String, String)> {
        let mut headers = Vec::new();
        if let Some(cached) = self.entries.get(url) {
            if let Some(ref etag) = cached.etag {
                headers.push(("If-None-Match".to_string(), etag.clone()));
            }
            if let Some(ref lm) = cached.last_modified {
                headers.push(("If-Modified-Since".to_string(), lm.clone()));
            }
        }
        headers
    }

    /// Number of cached entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Total cached bytes.
    pub fn total_bytes(&self) -> usize {
        self.entries.values().map(|e| e.body.len()).sum()
    }

    /// Clear all entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}
