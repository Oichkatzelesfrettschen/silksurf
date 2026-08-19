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

use rustc_hash::{FxHashMap, FxHasher};
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/*
 * Storable status codes (RFC 9111, 3, via the heuristically cacheable set in
 * 4.2.2). A status outside this set carries no reusable representation, so
 * storing it turns a transient server answer into the page a later navigation
 * renders: a 426 from chatgpt.com persisted here and served a blank document on
 * every subsequent load until the file was deleted by hand.
 */
const STORABLE_STATUS: &[u16] = &[200, 203, 204, 206, 300, 301, 308, 404, 405, 410, 414, 501];

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
    /// Seconds since the Unix epoch at which the response was stored. An entry
    /// written before this field existed decodes as 0, which reads as expired
    /// and forces a refetch.
    #[serde(default)]
    stored_at_unix: u64,
    /// Seconds the stored response stays fresh, from `Cache-Control: max-age`
    /// or `Expires`.
    #[serde(default)]
    freshness_secs: u64,
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
    /// Seconds this entry stays fresh, counted from `cached_at`.
    pub freshness_secs: u64,
    /// Seconds of freshness already spent when the entry loaded from disk.
    pub prior_age_secs: u64,
    /// URL that produced this response
    pub url: String,
}

impl CachedResponse {
    /// True while the stored response may be reused without revalidation
    /// (RFC 9111, 4.2).
    #[must_use]
    pub fn is_fresh(&self) -> bool {
        self.prior_age_secs
            .saturating_add(self.cached_at.elapsed().as_secs())
            < self.freshness_secs
    }
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

    /// Get a cached response for the given URL while it is fresh.
    ///
    /// A stale entry stays in the map so `conditional_headers` can still offer
    /// its validators; it just stops answering navigations on its own.
    #[must_use]
    pub fn get(&self, url: &str) -> Option<&CachedResponse> {
        self.entries.get(url).filter(|entry| entry.is_fresh())
    }

    /*
     * put -- store a response in the in-memory cache and optionally on disk.
     *
     * A response the origin marked no-store or private, or one whose status
     * carries no reusable representation, is dropped rather than stored:
     * `response_is_storable` decides, and a rejected response also evicts any
     * entry the URL already held, so the cache never answers with a
     * representation the origin has replaced.
     *
     * Disk write is best-effort: errors are silently ignored so a read-only
     * filesystem does not prevent in-memory caching from working.
     */
    pub fn put(&mut self, url: String, response: &super::HttpResponse) {
        if !response_is_storable(response) {
            self.entries.remove(&url);
            if let Some(dir) = self.disk_dir.clone() {
                remove_from_disk(&dir, &url);
            }
            return;
        }
        let etag = response
            .header("etag")
            .map(std::string::ToString::to_string);
        let last_modified = response
            .header("last-modified")
            .map(std::string::ToString::to_string);

        let entry = CachedResponse {
            body: response.body.clone(),
            status: response.status,
            headers: response.headers.clone(),
            etag,
            last_modified,
            cached_at: Instant::now(),
            freshness_secs: freshness_lifetime(&response.headers),
            prior_age_secs: 0,
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
                freshness_secs: disk.freshness_secs,
                // Freshness is counted from the write, not from this load, so a
                // process restart does not reset an entry's age.
                prior_age_secs: unix_now().saturating_sub(disk.stored_at_unix),
                url: url.clone(),
            };
            if !disk_entry_is_decoded_text(&cached) {
                continue;
            }
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
 * INVARIANT: only text responses are stored. Binary responses stay in the
 * in-memory cache and never pass through UTF-8 JSON serialization.
 */
fn put_to_disk(dir: &Path, url: &str, entry: &CachedResponse) {
    let Some(body_utf8) = disk_cache_body(entry) else {
        return;
    };
    let mut hasher = FxHasher::default();
    url.hash(&mut hasher);
    let key = hasher.finish();

    let disk = CachedResponseDisk {
        url: url.to_string(),
        status: entry.status,
        body_utf8,
        headers: entry.headers.clone(),
        etag: entry.etag.clone(),
        last_modified: entry.last_modified.clone(),
        stored_at_unix: unix_now(),
        freshness_secs: entry.freshness_secs,
    };

    let Ok(json) = serde_json::to_vec(&disk) else {
        return;
    };
    let _ = std::fs::create_dir_all(dir);
    let path = dir.join(format!("{key:016x}.json"));
    let _ = std::fs::write(path, json);
}

fn disk_cache_body(entry: &CachedResponse) -> Option<String> {
    if !disk_entry_is_decoded_text(entry) {
        return None;
    }
    String::from_utf8(entry.body.clone()).ok()
}

fn disk_entry_is_decoded_text(entry: &CachedResponse) -> bool {
    is_text_response(entry)
        && header_value(&entry.headers, "transfer-encoding").is_none()
        && header_value(&entry.headers, "content-encoding").is_none()
}

fn is_text_response(entry: &CachedResponse) -> bool {
    let Some(content_type) = header_value(&entry.headers, "content-type") else {
        return false;
    };
    let media_type = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    media_type.starts_with("text/")
        || matches!(
            media_type.as_str(),
            "application/javascript" | "application/json" | "application/xml"
        )
}

fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn disk_entry_path(dir: &Path, url: &str) -> PathBuf {
    let mut hasher = FxHasher::default();
    url.hash(&mut hasher);
    dir.join(format!("{:016x}.json", hasher.finish()))
}

fn remove_from_disk(dir: &Path, url: &str) {
    let _ = std::fs::remove_file(disk_entry_path(dir, url));
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/*
 * response_is_storable -- RFC 9111, 3.
 *
 * The origin's directives decide first: `no-store` forbids keeping the
 * response at all, and `private` marks it for a single user's browser cache
 * that this shared on-disk store is not. What remains is storable only when
 * the status names a reusable representation.
 */
fn response_is_storable(response: &super::HttpResponse) -> bool {
    let directives = cache_control_directives(&response.headers);
    if directives.iter().any(|d| d == "no-store" || d == "private") {
        return false;
    }
    STORABLE_STATUS.contains(&response.status)
}

/*
 * freshness_lifetime -- RFC 9111, 4.2.1.
 *
 * `max-age` wins over `Expires`. `no-cache` stores the response but requires
 * revalidation before reuse, which is a zero-second lifetime. A response
 * naming neither also gets zero: heuristic freshness would let this cache
 * answer navigations from a representation no origin promised was reusable.
 */
fn freshness_lifetime(headers: &[(String, String)]) -> u64 {
    let directives = cache_control_directives(headers);
    if directives.iter().any(|d| d == "no-cache") {
        return 0;
    }
    for directive in &directives {
        if let Some(value) = directive.strip_prefix("max-age=") {
            return value.trim_matches('"').parse().unwrap_or(0);
        }
    }
    let (Some(expires), Some(date)) = (
        header_value(headers, "expires").and_then(parse_http_date),
        header_value(headers, "date").and_then(parse_http_date),
    ) else {
        return 0;
    };
    expires.saturating_sub(date)
}

fn cache_control_directives(headers: &[(String, String)]) -> Vec<String> {
    header_value(headers, "cache-control")
        .map(|value| {
            value
                .split(',')
                .map(|d| d.trim().to_ascii_lowercase())
                .collect()
        })
        .unwrap_or_default()
}

/*
 * parse_http_date -- IMF-fixdate to seconds since the Unix epoch.
 *
 * HTTP-date is fixed-width in the preferred form RFC 9110, 5.6.7 requires an
 * origin to send: `Sun, 06 Nov 1994 08:49:37 GMT`. The obsolete RFC 850 and
 * asctime forms return None, which reads as a zero freshness lifetime.
 */
fn parse_http_date(value: &str) -> Option<u64> {
    let text = value.trim();
    let rest = text.split_once(", ")?.1;
    let mut parts = rest.split(' ');
    let day: u64 = parts.next()?.parse().ok()?;
    let month = match parts.next()? {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    };
    let year: u64 = parts.next()?.parse().ok()?;
    let mut clock = parts.next()?.split(':');
    let hour: u64 = clock.next()?.parse().ok()?;
    let minute: u64 = clock.next()?.parse().ok()?;
    let second: u64 = clock.next()?.parse().ok()?;
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second)
}

/*
 * days_from_civil -- Howard Hinnant's civil-to-days algorithm.
 *
 * The era arithmetic folds the 400-year leap cycle into one division, so the
 * conversion needs no table and no loop. Dates before 1970 are outside the
 * range an HTTP freshness calculation reads, so the function saturates at the
 * epoch.
 */
fn days_from_civil(year: u64, month: u64, day: u64) -> u64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days_since_civil_epoch = era * 146_097 + day_of_era;
    days_since_civil_epoch.saturating_sub(719_468)
}

#[cfg(test)]
mod tests {
    use super::{CachedResponseDisk, ResponseCache, freshness_lifetime, parse_http_date, unix_now};
    use crate::HttpResponse;
    use std::path::PathBuf;

    fn text_response(status: u16, cache_control: &str) -> HttpResponse {
        let mut headers = vec![("Content-Type".to_string(), "text/html".to_string())];
        if !cache_control.is_empty() {
            headers.push(("Cache-Control".to_string(), cache_control.to_string()));
        }
        HttpResponse {
            status,
            headers,
            body: b"<p>body</p>".to_vec(),
        }
    }

    #[test]
    fn an_error_status_is_not_stored() {
        let mut cache = ResponseCache::new();
        cache.put(
            "https://example.test/".to_string(),
            &text_response(426, "max-age=600"),
        );
        assert!(cache.get("https://example.test/").is_none());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn a_storable_error_status_is_kept() {
        let mut cache = ResponseCache::new();
        cache.put(
            "https://example.test/missing".to_string(),
            &text_response(404, "max-age=600"),
        );
        assert!(cache.get("https://example.test/missing").is_some());
    }

    #[test]
    fn no_store_and_private_responses_are_dropped() {
        for directive in ["no-store", "private, max-age=600"] {
            let mut cache = ResponseCache::new();
            cache.put(
                "https://example.test/".to_string(),
                &text_response(200, directive),
            );
            assert!(
                cache.get("https://example.test/").is_none(),
                "stored a {directive} response"
            );
        }
    }

    #[test]
    fn an_uncacheable_response_evicts_the_entry_it_replaces() {
        let mut cache = ResponseCache::new();
        cache.put(
            "https://example.test/".to_string(),
            &text_response(200, "max-age=600"),
        );
        assert!(cache.get("https://example.test/").is_some());
        cache.put("https://example.test/".to_string(), &text_response(503, ""));
        assert!(cache.get("https://example.test/").is_none());
    }

    #[test]
    fn a_response_without_a_lifetime_does_not_answer_a_later_navigation() {
        let mut cache = ResponseCache::new();
        cache.put("https://example.test/".to_string(), &text_response(200, ""));
        assert!(cache.get("https://example.test/").is_none());
        // The entry survives for its validators even though it cannot be reused.
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn no_cache_stores_the_response_with_a_zero_lifetime() {
        let mut cache = ResponseCache::new();
        cache.put(
            "https://example.test/".to_string(),
            &text_response(200, "no-cache, max-age=600"),
        );
        assert!(cache.get("https://example.test/").is_none());
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn max_age_wins_over_expires() {
        let headers = vec![
            ("Cache-Control".to_string(), "max-age=42".to_string()),
            (
                "Expires".to_string(),
                "Sun, 06 Nov 1994 08:49:37 GMT".to_string(),
            ),
            (
                "Date".to_string(),
                "Sun, 06 Nov 1994 08:49:37 GMT".to_string(),
            ),
        ];
        assert_eq!(freshness_lifetime(&headers), 42);
    }

    #[test]
    fn expires_minus_date_sets_the_lifetime() {
        let headers = vec![
            (
                "Date".to_string(),
                "Sun, 06 Nov 1994 08:49:37 GMT".to_string(),
            ),
            (
                "Expires".to_string(),
                "Sun, 06 Nov 1994 09:49:37 GMT".to_string(),
            ),
        ];
        assert_eq!(freshness_lifetime(&headers), 3_600);
    }

    #[test]
    fn imf_fixdate_parses_to_the_documented_epoch_seconds() {
        // RFC 9110, 5.6.7 gives this date as the preferred-form example.
        assert_eq!(
            parse_http_date("Sun, 06 Nov 1994 08:49:37 GMT"),
            Some(784_111_777)
        );
        assert_eq!(parse_http_date("Thu, 01 Jan 1970 00:00:00 GMT"), Some(0));
        assert_eq!(parse_http_date("Sunday, 06-Nov-94 08:49:37 GMT"), None);
    }

    #[test]
    fn a_disk_entry_without_a_stored_time_reads_as_expired() {
        let dir = temp_cache_dir("legacy");
        std::fs::create_dir_all(&dir).expect("create cache dir");
        let json = br#"{"url":"https://example.test/old","status":200,
            "body_utf8":"<p>old</p>",
            "headers":[["Content-Type","text/html"]],
            "etag":null,"last_modified":null}"#;
        std::fs::write(dir.join("legacy.json"), json).expect("write cache row");
        let cache = ResponseCache::with_disk(&dir);
        assert_eq!(cache.len(), 1);
        assert!(cache.get("https://example.test/old").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn disk_cache_skips_binary_responses() {
        let dir = temp_cache_dir("binary");
        let mut cache = ResponseCache::with_disk(&dir);
        cache.put(
            "https://example.test/avatar.png".to_string(),
            &HttpResponse {
                status: 200,
                headers: vec![
                    ("Content-Type".to_string(), "image/png".to_string()),
                    ("Cache-Control".to_string(), "max-age=60".to_string()),
                ],
                body: vec![0x89, b'P', b'N', b'G', 0xff],
            },
        );

        let reloaded = ResponseCache::with_disk(&dir);

        assert!(reloaded.get("https://example.test/avatar.png").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn disk_cache_loads_text_responses() {
        let dir = temp_cache_dir("text");
        let mut cache = ResponseCache::with_disk(&dir);
        cache.put(
            "https://example.test/style.css".to_string(),
            &HttpResponse {
                status: 200,
                headers: vec![
                    ("Content-Type".to_string(), "text/css".to_string()),
                    ("Cache-Control".to_string(), "max-age=60".to_string()),
                ],
                body: b"body { color: black; }".to_vec(),
            },
        );

        let reloaded = ResponseCache::with_disk(&dir);

        assert_eq!(
            reloaded
                .get("https://example.test/style.css")
                .map(|entry| entry.body.as_slice()),
            Some(b"body { color: black; }".as_slice())
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn disk_cache_skips_transport_encoded_text_responses() {
        let dir = temp_cache_dir("encoded");
        std::fs::create_dir_all(&dir).expect("create cache dir");
        let disk = CachedResponseDisk {
            url: "https://example.test/".to_string(),
            status: 200,
            body_utf8: "3\r\nabc\r\n0\r\n\r\n".to_string(),
            headers: vec![
                ("Content-Type".to_string(), "text/html".to_string()),
                ("Transfer-Encoding".to_string(), "chunked".to_string()),
                ("Cache-Control".to_string(), "max-age=60".to_string()),
            ],
            etag: None,
            last_modified: None,
            stored_at_unix: unix_now(),
            freshness_secs: 60,
        };
        let json = serde_json::to_vec(&disk).expect("serialize cache row");
        std::fs::write(dir.join("encoded.json"), json).expect("write cache row");

        let reloaded = ResponseCache::with_disk(&dir);

        assert!(reloaded.get("https://example.test/").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    fn temp_cache_dir(label: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "silksurf-cache-test-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time is after epoch")
                .as_nanos()
        ));
        dir
    }
}
