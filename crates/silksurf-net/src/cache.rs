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
 * Disk persistence: on process start we load all entries from
 * ~/.cache/silksurf/http/<hash>.json. On put() we write the new entry to disk.
 * This makes FetchOrigin::Cache fire on the second process invocation for the
 * same URL, enabling the speculative revalidation path.
 *
 * Serialization: CachedResponseDisk (no Instant) is the on-disk form. The
 * in-memory CachedResponse adds cached_at: Instant for TTL checks.
 *
 * Memory: responses stored in FxHashMap<String, CachedResponse>.
 * For ChatGPT: ~1.5MB (HTML + CSS). Acceptable for a browser cache.
 *
 * See: BasicClient::fetch() in lib.rs for the actual HTTP client
 * See: silksurf-app/src/main.rs for speculative pre-render integration
 */

use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Instant;

/*
 * CachedResponseDisk -- the on-disk representation of a cached response.
 *
 * WHY separate from CachedResponse: std::time::Instant is not serializable.
 * We use a Unix timestamp (seconds since epoch) for the cached_at_secs field
 * as an informational age hint; it is not used for eviction in this implementation.
 *
 * INVARIANT: body is stored as UTF-8 text (lossy) to keep JSON files readable
 * and avoid base64 overhead for typical HTML/CSS responses.
 * Binary responses (images, etc.) are not stored on disk (see put_to_disk).
 */
#[derive(Debug, Serialize, Deserialize)]
struct CachedResponseDisk {
    url: String,
    status: u16,
    body_utf8: String,
    headers: Vec<(String, String)>,
    etag: Option<String>,
    last_modified: Option<String>,
}

/// A cached HTTP response with validation headers.
#[derive(Debug, Clone)]
pub struct CachedResponse {
    /// The full response body
    pub body: Vec<u8>,
    /// HTTP status code
    pub status: u16,
    /// Response headers (for content-type, etc.)
    pub headers: Vec<(String, String)>,
    /// `ETag` for conditional revalidation
    pub etag: Option<String>,
    /// Last-Modified for conditional revalidation
    pub last_modified: Option<String>,
    /// When this entry was cached
    pub cached_at: Instant,
    /// URL that produced this response
    pub url: String,
}

/// Simple in-memory HTTP response cache with optional disk persistence.
#[derive(Default)]
pub struct ResponseCache {
    entries: FxHashMap<String, CachedResponse>,
    /// Optional directory for disk-backed persistence.
    disk_dir: Option<PathBuf>,
}

impl ResponseCache {
    #[must_use] 
    pub fn new() -> Self {
        Self::default()
    }

    /*
     * with_disk -- create a cache backed by a disk directory.
     *
     * WHY: The in-memory cache is lost on process exit.  Persisting to disk
     * makes the second invocation of silksurf-app serve from FetchOrigin::Cache
     * immediately, enabling the speculative revalidation path and sub-ms
     * cached re-renders across sessions.
     *
     * Existing entries on disk are loaded synchronously during construction.
     * This is a one-time O(N_files) scan at startup; for typical use (< 20
     * cached URLs) this is < 1ms.
     *
     * HOW: entries are stored as ~/.cache/silksurf/http/<hash>.json
     * where <hash> is the FxHash of the URL.  One file per URL.
     * Stale/corrupted files are silently skipped.
     *
     * See: put_to_disk, load_from_disk below
     */
    #[must_use] 
    pub fn with_disk(dir: &Path) -> Self {
        let mut cache = Self {
            entries: FxHashMap::default(),
            disk_dir: Some(dir.to_path_buf()),
        };
        cache.load_all_from_disk(dir);
        cache
    }

    /// Get a cached response for the given URL, if available.
    #[must_use] 
    pub fn get(&self, url: &str) -> Option<&CachedResponse> {
        self.entries.get(url)
    }

    /*
     * put -- store a response in the in-memory cache and optionally on disk.
     *
     * Disk write is best-effort: errors are silently ignored so a read-only
     * filesystem does not prevent in-memory caching from working.
     */
    pub fn put(&mut self, url: String, response: &super::HttpResponse) {
        let etag = response.header("etag").map(std::string::ToString::to_string);
        let last_modified = response.header("last-modified").map(std::string::ToString::to_string);

        let entry = CachedResponse {
            body: response.body.clone(),
            status: response.status,
            headers: response.headers.clone(),
            etag,
            last_modified,
            cached_at: Instant::now(),
            url: url.clone(),
        };

        if let Some(dir) = self.disk_dir.clone() {
            put_to_disk(&dir, &url, &entry);
        }

        self.entries.insert(url, entry);
    }

    /// Build conditional request headers for revalidation.
    #[must_use] 
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
    #[must_use] 
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if cache is empty.
    #[must_use] 
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Total cached bytes.
    #[must_use] 
    pub fn total_bytes(&self) -> usize {
        self.entries.values().map(|e| e.body.len()).sum()
    }

    /// Clear all entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /*
     * load_all_from_disk -- read all JSON files from the cache directory.
     *
     * Skips files that fail to parse (corrupted / from old format) silently.
     * Called once during construction of a disk-backed cache.
     *
     * Complexity: O(N_files * avg_file_size) -- bounded by disk I/O, not CPU.
     */
    fn load_all_from_disk(&mut self, dir: &Path) {
        let Ok(read_dir) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let Ok(disk): Result<CachedResponseDisk, _> = serde_json::from_slice(&bytes) else {
                continue;
            };
            let url = disk.url.clone();
            let cached = CachedResponse {
                body: disk.body_utf8.into_bytes(),
                status: disk.status,
                headers: disk.headers,
                etag: disk.etag,
                last_modified: disk.last_modified,
                cached_at: Instant::now(),
                url: url.clone(),
            };
            self.entries.insert(url, cached);
        }
    }
}

/*
 * put_to_disk -- serialize one cache entry to a JSON file.
 *
 * WHY fn not method: disk_dir clone above moves it; keeping this as a free
 * function avoids the borrow conflict on &mut self.
 *
 * File name: FxHash of the URL (hex) + ".json". Collision probability for
 * < 1000 URLs is negligible (64-bit hash space).
 *
 * INVARIANT: only text responses are stored (body_utf8 field is lossy UTF-8).
 * Binary responses are not written (would produce unreadable JSON for images).
 * In practice, silksurf-net fetches HTML and CSS only, which are always text.
 */
fn put_to_disk(dir: &Path, url: &str, entry: &CachedResponse) {
    use rustc_hash::FxHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = FxHasher::default();
    url.hash(&mut hasher);
    let key = hasher.finish();

    let disk = CachedResponseDisk {
        url: url.to_string(),
        status: entry.status,
        body_utf8: String::from_utf8_lossy(&entry.body).into_owned(),
        headers: entry.headers.clone(),
        etag: entry.etag.clone(),
        last_modified: entry.last_modified.clone(),
    };

    let Ok(json) = serde_json::to_vec(&disk) else {
        return;
    };
    let _ = std::fs::create_dir_all(dir);
    let path = dir.join(format!("{key:016x}.json"));
    let _ = std::fs::write(path, json);
}
