/*
 * speculative.rs -- speculative pre-render orchestrator.
 *
 * WHY: Sub-millisecond re-navigation requires zero network latency.
 * This module implements cache-first fetching with background revalidation:
 *   1. On first visit: fetch live, store in ResponseCache
 *   2. On re-visit: serve from cache immediately (0ms network time)
 *   3. Spawn background thread with conditional GET (If-None-Match / If-Modified-Since)
 *   4. If 304 Not Modified: cached render stays valid, no work needed
 *   5. If 200: new content, caller re-renders the delta
 *
 * Analogy to LBM perturbation (gororoba): the cached response is the
 * equilibrium f_eq; the delta from a 200 revalidation is the perturbation h.
 * We only reprocess h when it is nonzero (i.e., content changed).
 *
 * Thread model: SpeculativeRenderer is !Send (ResponseCache uses FxHashMap).
 * The background revalidation thread gets its own Arc<BasicClient> and a
 * clone of the conditional headers; it sends the result via std::sync::mpsc.
 *
 * Complexity: O(1) cache lookup, O(network) on first fetch and revalidation
 * Memory: O(responses) in ResponseCache -- one entry per URL
 *
 * See: silksurf-net/src/cache.rs ResponseCache for entry format
 * See: silksurf-net/src/lib.rs BasicClient for HTTP/1.1 + TLS client
 * See: silksurf-app/src/main.rs for integration point
 */

use rustc_hash::FxHashMap;
use silksurf_core::SilkInterner;
use silksurf_css::{
    Stylesheet, intern_rules, parse_stylesheet_with_interner, strip_selector_atoms,
};
use silksurf_net::cache::ResponseCache;
use silksurf_net::{BasicClient, HttpMethod, HttpRequest, HttpResponse, NetClient, NetError};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Instant;

/*
 * FetchOrigin -- tracks whether a response came from the cache or the network.
 *
 * WHY: Callers need to know whether they are rendering a speculative (cached)
 * frame or a fresh one, so they can decide whether to start background
 * revalidation and whether to show a "stale" indicator.
 */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchOrigin {
    /// Response was fetched live from the server and is now cached.
    Fresh,
    /// Response was served from the local cache without a network round-trip.
    Cache,
}

/*
 * RevalidationResult -- outcome of a background conditional GET.
 *
 * changed=false means the server returned 304 Not Modified; the cached
 * render is still valid and no re-render is needed.
 *
 * changed=true means the server returned 200 with new content. The caller
 * should re-render using response.unwrap() and update the cache.
 */
pub struct RevalidationResult {
    /// True if the server returned 200 (content changed since last cache).
    pub changed: bool,
    /// The new response body, present only when changed=true.
    pub response: Option<HttpResponse>,
    /// Round-trip time for the background revalidation request.
    pub rtt: std::time::Duration,
}

/*
 * RevalidationHandle -- non-blocking handle to a background revalidation.
 *
 * WHY: The background thread runs a full HTTP round-trip. The handle lets the
 * caller continue with rendering the cached page while the network request is
 * in flight. Call wait() after rendering to apply any delta.
 */
pub struct RevalidationHandle {
    rx: mpsc::Receiver<Result<RevalidationResult, NetError>>,
    url: String,
}

impl RevalidationHandle {
    /*
     * wait -- block until the revalidation completes.
     *
     * Returns Err if the background thread panicked or the network failed.
     * Returns Ok(result) with result.changed indicating whether a re-render
     * is needed.
     */
    pub fn wait(self) -> Result<RevalidationResult, NetError> {
        self.rx.recv().unwrap_or_else(|_| {
            Err(NetError {
                message: "revalidation thread panicked".to_string(),
            })
        })
    }

    /*
     * try_recv -- non-blocking poll: return Some if done, None if still in flight.
     *
     * WHY: Lets the caller render from cache and check for an update only when
     * the revalidation has already completed (zero additional latency).
     */
    pub fn try_recv(&self) -> Option<Result<RevalidationResult, NetError>> {
        self.rx.try_recv().ok()
    }

    /// The URL being revalidated.
    pub fn url(&self) -> &str {
        &self.url
    }
}

/*
 * StylesheetCache -- maps CSS text hash -> parsed-but-uninternalized Stylesheet.
 *
 * WHY: Parsing 128KB of ChatGPT CSS costs 2.5ms per render even when the
 * bytes are identical to the previous render. This cache stores the parsed
 * Stylesheet (with SmallString selectors, atom=None) and avoids re-parsing.
 *
 * On cache HIT:  clone Arc<Stylesheet> + call intern_rules (~100-200us)
 * On cache MISS: full parse (2.5ms) + strip_selector_atoms + store Arc
 *
 * KEY INVARIANT: stored Stylesheets have atom=None in all SelectorIdents.
 * Interning is done per-render against the current DOM's interner, so each
 * render gets atoms valid for its own interner without cross-contamination.
 *
 * Complexity: O(1) FxHashMap lookup on hit + O(N_selectors) intern_rules
 * See: intern_rules, strip_selector_atoms in silksurf-css/src/selector.rs
 * See: SelectorIdent.clear_atom, SelectorIdent.intern_with in selector.rs
 */
struct StylesheetCache {
    entries: FxHashMap<u64, Arc<Stylesheet>>,
}

impl StylesheetCache {
    fn new() -> Self {
        Self {
            entries: FxHashMap::default(),
        }
    }
}

fn hash_css_text(css: &str) -> u64 {
    let mut h = rustc_hash::FxHasher::default();
    css.as_bytes().hash(&mut h);
    h.finish()
}

/*
 * disk_cache_path -- return the file path for a stylesheet disk cache entry.
 *
 * WHY: The in-memory StylesheetCache is lost on process exit. The disk cache
 * persists across process restarts so cold starts pay only ~200us for
 * intern_rules instead of ~2.5ms for a full CSS parse.
 *
 * Format: {tmpdir}/silksurf_css_cache/{key:016x}.bin
 * Key:    FxHash(css_text) -- 64-bit, 16 hex chars, collision probability ~1e-9
 *
 * See: load_stylesheet_from_disk, save_stylesheet_to_disk below
 */
fn disk_cache_path(key: u64) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push("silksurf_css_cache");
    p.push(format!("{key:016x}.bin"));
    p
}

/*
 * load_stylesheet_from_disk -- try to deserialize a cached Stylesheet.
 *
 * Returns None on any error (file not found, corrupt bytes, format version
 * mismatch). Callers fall through to full parse on None.
 *
 * WHY graceful fallback: serialization format changes between releases would
 * otherwise cause panics. By catching errors here, we silently re-parse
 * and overwrite stale caches.
 */
fn load_stylesheet_from_disk(path: &PathBuf) -> Option<Stylesheet> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice::<Stylesheet>(&bytes).ok()
}

/*
 * save_stylesheet_to_disk -- serialize an uninternalized Stylesheet to disk.
 *
 * WHY: called once per cache miss so subsequent process restarts pay only
 * ~200us (disk read + intern_rules) instead of ~2.5ms (full parse).
 * Errors are silently ignored -- the in-memory cache still works.
 */
fn save_stylesheet_to_disk(path: &PathBuf, sheet: &Stylesheet) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(bytes) = serde_json::to_vec(sheet) {
        let _ = std::fs::write(path, bytes);
    }
}

/*
 * SpeculativeRenderer -- cache-first HTTP client with background revalidation.
 *
 * On first fetch: goes to the network, stores response in ResponseCache.
 * On subsequent fetches: returns cached response immediately, and the caller
 * can optionally spawn a background conditional GET to check for updates.
 *
 * INVARIANT: cache always reflects the most recent 200 response for each URL.
 * A 304 revalidation does NOT update the cache (content unchanged).
 *
 * WHY Arc<BasicClient>: the background revalidation thread needs a client.
 * BasicClient is Send+Sync (its only non-trivial field is Arc<dyn TlsProvider
 * + Send+Sync>), so it is safe to share across threads via Arc.
 */
pub struct SpeculativeRenderer {
    pub cache: ResponseCache,
    client: Arc<BasicClient>,
    stylesheet_cache: StylesheetCache,
}

type FetchResult = Result<(HttpResponse, FetchOrigin, std::time::Duration), NetError>;

impl SpeculativeRenderer {
    pub fn new() -> Self {
        Self {
            cache: ResponseCache::new(),
            client: Arc::new(BasicClient::new()),
            stylesheet_cache: StylesheetCache::new(),
        }
    }

    /*
     * with_insecure -- constructor that disables TLS certificate verification.
     *
     * WHY: Some development environments have broken cert chains. This allows
     * testing with self-signed certs without changing the production code path.
     * NEVER use in production.
     */
    pub fn with_insecure() -> Self {
        use silksurf_tls::RustlsProvider;
        Self {
            cache: ResponseCache::new(),
            client: Arc::new(BasicClient::with_tls(Arc::new(
                RustlsProvider::new_insecure(),
            ))),
            stylesheet_cache: StylesheetCache::new(),
        }
    }

    /*
     * with_extra_ca_file -- constructor that adds a user-supplied PEM CA bundle
     * to the default Mozilla + native root store.
     *
     * WHY: Corporate proxies and private PKI deployments present certificates
     * signed by an internal CA that is not in Mozilla's root bundle. Rather
     * than disabling all verification (--insecure), the user can supply the
     * specific CA cert file so only that chain is trusted additionally.
     *
     * Error: returns Err if the file cannot be opened, contains no parseable
     * PEM certificates, or rustls rejects all of them.
     */
    pub fn with_extra_ca_file(path: &std::path::Path) -> Result<Self, NetError> {
        use silksurf_tls::RustlsProvider;

        let provider =
            RustlsProvider::new_with_extra_ca_file(path).map_err(|e| NetError {
                message: format!("TLS CA file {}: {e}", path.display()),
            })?;

        Ok(Self {
            cache: ResponseCache::new(),
            client: Arc::new(BasicClient::with_tls(Arc::new(provider))),
            stylesheet_cache: StylesheetCache::new(),
        })
    }

    /*
     * with_platform_verifier -- constructor that asks rustls to use the best
     * platform verifier available for this target.
     *
     * On Linux this still uses WebPKI over the discovered native root bundle;
     * on Windows/macOS/mobile it can use the OS verifier and its richer trust
     * policy.
     */
    #[cfg(feature = "platform-verifier")]
    pub fn with_platform_verifier() -> Result<Self, NetError> {
        use silksurf_tls::RustlsProvider;

        let provider = RustlsProvider::new_platform_verifier().map_err(|e| NetError {
            message: format!("TLS platform verifier setup: {e}"),
        })?;

        Ok(Self {
            cache: ResponseCache::new(),
            client: Arc::new(BasicClient::with_tls(Arc::new(provider))),
            stylesheet_cache: StylesheetCache::new(),
        })
    }

    /*
     * get_or_parse_stylesheet -- CSS parse with in-process result caching.
     *
     * WHY: Calling parse_stylesheet_with_interner on every render costs 2.5ms
     * for ChatGPT's 128KB CSS even when the bytes have not changed. This method
     * caches the Stylesheet keyed by FxHash(css_text) so subsequent renders pay
     * only ~100-200us for intern_rules rather than the full tokenize+parse cost.
     *
     * Algorithm:
     *   1. Hash css_text in O(N_bytes) -- ~2us for 128KB
     *   2. On HIT: clone Arc<Stylesheet> + call intern_rules -- ~100-200us
     *   3. On MISS: full parse -- ~2.5ms; strip atoms; store Arc; return
     *
     * INVARIANT: the returned Stylesheet has atom=Some for all selector idents
     * that pass should_intern_identifier(). Atoms are valid for `interner`.
     *
     * See: StylesheetCache invariant above for why we strip atoms before caching.
     * See: intern_rules, strip_selector_atoms in silksurf-css/src/selector.rs
     *
     * Complexity: O(N_bytes) hash + O(N_selectors) intern on hit;
     *             O(N_tokens) parse + O(N_selectors) strip on miss.
     */
    pub fn get_or_parse_stylesheet(
        &mut self,
        css_text: &str,
        interner: &mut SilkInterner,
    ) -> Option<Stylesheet> {
        let key = hash_css_text(css_text);

        if let Some(cached) = self.stylesheet_cache.entries.get(&key) {
            // Cache hit: clone the uninternalized stylesheet and re-intern.
            let mut sheet = (**cached).clone();
            intern_rules(&mut sheet.rules, interner);
            return Some(sheet);
        }

        // Cache miss: try disk cache before full parse.
        //
        // WHY: full parse costs ~2.5ms; disk read + JSON decode + intern_rules
        // costs ~200us. On cold process start the in-memory cache is empty but the disk
        // cache may have a valid serialized Stylesheet from a previous run.
        //
        // INVARIANT: deserialized Stylesheets have atom=None (#[serde(skip)]).
        // We populate atoms by running intern_rules before returning.
        let path = disk_cache_path(key);
        if let Some(mut disk_sheet) = load_stylesheet_from_disk(&path) {
            intern_rules(&mut disk_sheet.rules, interner);
            // Populate in-memory cache so subsequent renders skip disk I/O entirely.
            let mut uninit = disk_sheet.clone();
            strip_selector_atoms(&mut uninit.rules);
            self.stylesheet_cache.entries.insert(key, Arc::new(uninit));
            return Some(disk_sheet);
        }

        // Full parse (both in-memory and disk caches missed).
        let sheet = parse_stylesheet_with_interner(css_text, interner).ok()?;

        // Strip atoms for storage (SmallStrings retained for equality fallback).
        let mut uninit = sheet.clone();
        strip_selector_atoms(&mut uninit.rules);

        // Persist to disk so subsequent process restarts pay only ~200us.
        save_stylesheet_to_disk(&path, &uninit);

        self.stylesheet_cache.entries.insert(key, Arc::new(uninit));

        Some(sheet)
    }

    /*
     * fetch_or_speculate -- cache-first fetch.
     *
     * Returns the cached response immediately if available (FetchOrigin::Cache),
     * otherwise performs a live fetch, caches the result, and returns it
     * (FetchOrigin::Fresh).
     *
     * The extra_headers slice is appended to both live and cached responses
     * (for the live path only -- cached responses were stored with their
     * original headers).
     *
     * Complexity: O(1) cache lookup + O(network) on first fetch
     */
    pub fn fetch_or_speculate(
        &mut self,
        url: &str,
        extra_headers: &[(String, String)],
    ) -> Result<(HttpResponse, FetchOrigin, std::time::Duration), NetError> {
        let t0 = Instant::now();

        // Cache hit: return immediately without any network I/O
        if let Some(cached) = self.cache.get(url) {
            let response = HttpResponse {
                status: cached.status,
                headers: cached.headers.clone(),
                body: cached.body.clone(),
            };
            return Ok((response, FetchOrigin::Cache, t0.elapsed()));
        }

        // Cache miss: fetch live, store in cache
        let mut headers = extra_headers.to_vec();
        headers.push(("Accept".to_string(), "text/html,*/*".to_string()));
        headers.push((
            "User-Agent".to_string(),
            "SilkSurf/0.1 (X11; Linux x86_64)".to_string(),
        ));

        let request = HttpRequest {
            method: HttpMethod::Get,
            url: url.to_string(),
            headers,
            body: Vec::new(),
        };

        let response = self.client.fetch(&request)?;
        self.cache.put(url.to_string(), &response);
        Ok((response, FetchOrigin::Fresh, t0.elapsed()))
    }

    /*
     * fetch_all_or_speculate -- cache-first parallel fetch for multiple URLs.
     *
     * WHY: CSS subresources for a page (e.g. chatgpt.com's 2 stylesheets) are
     * currently fetched sequentially. This method:
     *   1. Returns cached responses immediately for URLs already in the cache
     *   2. Groups uncached URLs and fetches them via BasicClient::fetch_parallel,
     *      which uses HTTP/2 multiplexing when all URLs share an HTTPS host
     *   3. Stores new responses in the cache
     *
     * Result order matches the input order (same-index correspondence).
     *
     * INVARIANT: each (url, extra_headers) pair in `requests` produces exactly
     * one result in the returned Vec at the same index.
     *
     * Complexity: O(cached) = O(1) lookups; O(uncached) = O(1) TLS + O(N) frames
     * See: BasicClient::fetch_parallel for h2 implementation
     * See: silksurf-app/src/main.rs for call site
     */
    pub fn fetch_all_or_speculate(
        &mut self,
        requests: &[(&str, &[(String, String)])],
    ) -> Vec<FetchResult> {
        let t0 = Instant::now();
        let n = requests.len();
        let mut results: Vec<Option<FetchResult>> = vec![None; n];

        // Phase 1: serve cached URLs immediately.
        let mut uncached_indices: Vec<usize> = Vec::new();
        for (i, &(url, _)) in requests.iter().enumerate() {
            if let Some(cached) = self.cache.get(url) {
                results[i] = Some(Ok((
                    HttpResponse {
                        status: cached.status,
                        headers: cached.headers.clone(),
                        body: cached.body.clone(),
                    },
                    FetchOrigin::Cache,
                    t0.elapsed(),
                )));
            } else {
                uncached_indices.push(i);
            }
        }

        // Phase 2: fetch uncached in parallel via BasicClient::fetch_parallel.
        if !uncached_indices.is_empty() {
            let http_requests: Vec<HttpRequest> = uncached_indices
                .iter()
                .map(|&i| {
                    let (url, extra_headers) = requests[i];
                    let mut headers = extra_headers.to_vec();
                    headers.push(("Accept".to_string(), "text/html,text/css,*/*".to_string()));
                    headers.push((
                        "User-Agent".to_string(),
                        "SilkSurf/0.1 (X11; Linux x86_64)".to_string(),
                    ));
                    HttpRequest {
                        method: HttpMethod::Get,
                        url: url.to_string(),
                        headers,
                        body: Vec::new(),
                    }
                })
                .collect();

            let responses = self.client.fetch_parallel(&http_requests);

            for (j, &idx) in uncached_indices.iter().enumerate() {
                let url = requests[idx].0;
                match &responses[j] {
                    Ok(resp) => {
                        self.cache.put(url.to_string(), resp);
                        results[idx] = Some(Ok((resp.clone(), FetchOrigin::Fresh, t0.elapsed())));
                    }
                    Err(e) => {
                        results[idx] = Some(Err(e.clone()));
                    }
                }
            }
        }

        results.into_iter().map(|r| r.unwrap()).collect()
    }

    /*
     * spawn_revalidation -- start a background conditional GET.
     *
     * Reads the ETag / Last-Modified from the cache entry for `url`, builds
     * If-None-Match / If-Modified-Since headers, and spawns a std::thread to
     * perform the conditional fetch. Returns a RevalidationHandle immediately.
     *
     * The caller renders from cache while the thread is in flight, then calls
     * handle.wait() (or handle.try_recv()) to apply any delta.
     *
     * WHY separate thread (not async): the app is single-threaded synchronous;
     * adding an async runtime just for this one use-case would increase
     * complexity more than it reduces latency. A single thread + mpsc gives
     * the same overlap at zero additional deps.
     *
     * INVARIANT: only call this after a successful fetch_or_speculate that
     * returned FetchOrigin::Cache. Calling on an uncached URL sends an
     * unconditional GET (no validation headers), which is wasteful.
     */
    pub fn spawn_revalidation(&self, url: &str) -> RevalidationHandle {
        /*
         * Clone conditional headers before spawning: ResponseCache is !Send
         * (FxHashMap is not Sync), so we cannot move the cache into the thread.
         * Instead we extract just the validation headers we need and clone them.
         */
        let cond_headers = self.cache.conditional_headers(url);
        let url_owned = url.to_string();
        let client = Arc::clone(&self.client);
        let (tx, rx) = mpsc::channel();

        std::thread::spawn(move || {
            let t0 = Instant::now();
            let mut headers = cond_headers;
            headers.push(("Accept".to_string(), "text/html,*/*".to_string()));
            headers.push((
                "User-Agent".to_string(),
                "SilkSurf/0.1 (X11; Linux x86_64)".to_string(),
            ));

            let request = HttpRequest {
                method: HttpMethod::Get,
                url: url_owned.clone(),
                headers,
                body: Vec::new(),
            };

            let result = client.fetch(&request).map(|response| {
                let rtt = t0.elapsed();
                if response.status == 304 {
                    // 304 Not Modified: cached content still valid
                    RevalidationResult {
                        changed: false,
                        response: None,
                        rtt,
                    }
                } else {
                    // 200 (or other): new content received
                    RevalidationResult {
                        changed: true,
                        response: Some(response),
                        rtt,
                    }
                }
            });

            let _ = tx.send(result);
        });

        RevalidationHandle {
            rx,
            url: url.to_string(),
        }
    }

    /*
     * update_cache -- store a revalidation 200 response back into the cache.
     *
     * Call this after handle.wait() returns RevalidationResult { changed: true }.
     * Keeps the cache consistent with the latest server content.
     */
    pub fn update_cache(&mut self, url: &str, response: &HttpResponse) {
        self.cache.put(url.to_string(), response);
    }

    /// Total bytes held in cache across all URLs.
    pub fn cache_bytes(&self) -> usize {
        self.cache.total_bytes()
    }
}

impl Default for SpeculativeRenderer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch_origin_debug() {
        assert_eq!(format!("{:?}", FetchOrigin::Fresh), "Fresh");
        assert_eq!(format!("{:?}", FetchOrigin::Cache), "Cache");
    }

    #[test]
    fn test_cache_empty_on_new() {
        let renderer = SpeculativeRenderer::new();
        assert_eq!(renderer.cache.len(), 0);
        assert_eq!(renderer.cache_bytes(), 0);
    }
}
