//! HTTP cookie primitives: parsing, storage, and header serialization.
//!
//! `Cookie` models one stored cookie with the attributes RFC 6265 defines
//! (domain, path, expiry, Secure, HttpOnly, SameSite). `CookieStore` holds a
//! set of cookies and answers two questions a browser asks: what `Cookie`
//! request header to send for a URL, and what `document.cookie` string a script
//! may read. `parse_set_cookie` turns a `Set-Cookie` response header into a
//! `Cookie`.
//!
//! Time is injected as `now_unix` (Unix seconds) into every function that needs
//! it, so expiry logic is deterministic and testable; `*_now` convenience
//! wrappers read the wall clock.
//!
//! Scope and honesty:
//! - Site derivation (`site_of_url`) consults the Public Suffix List via
//!   `silksurf_core::psl`, so a cookie partition and same-site comparison use
//!   the registrable domain (eTLD+1). The RFC 6265 `Domain=` *attribute* check
//!   is separate and still permissive: `set_from_set_cookie` does not yet
//!   reject `Domain=<public suffix>` (e.g. `Domain=.co.uk`), so a page cannot
//!   partition-escape, but the parser does not enforce the suffix rule on the
//!   attribute itself; that check is a follow-on.
//! - `SameSite` is parsed, stored, and enforced. Subresources are classified by
//!   `subresource_same_site_context` (cross-site subresources withhold
//!   Strict/Lax); top-level navigations by `navigation_same_site_context` from
//!   the initiator site (a cross-site navigation withholds Strict, and Lax too
//!   for an unsafe method). A path with no top-level site plumbed passes
//!   `SameSiteContext::Unknown`, applying no filtering (graceful degradation).

use std::time::{SystemTime, UNIX_EPOCH};

/// Synthetic host under which `document.cookie` cookies are stored. Reads
/// ignore host/path, so the value only needs to be stable, not real.
const DOCUMENT_JAR_HOST: &str = "document.jar.invalid";

/// Partition key used when the top-level site is unknown (not plumbed). Cookies
/// land in one shared, unpartitioned store and enforcement is not applied --
/// the graceful degradation when a fetch path did not receive the top-level
/// site. Distinct from any real `resource^top` key.
pub const UNPARTITIONED: &str = "<unpartitioned>";

/// The site (scheme://registrable-domain, port-independent) of a URL, or `null`
/// for a URL with no host. This is the site half of a cookie partition key and
/// the unit of same-site comparison.
///
/// The host is reduced to its registrable domain (eTLD+1) via the Public Suffix
/// List, so `a.example.com` and `b.example.com` share a site but `a.co.uk` and
/// `b.co.uk` do not. A host with no registrable domain (an IP literal, a bare
/// public suffix, `localhost`) keeps its full host, which is maximally
/// partitioned.
#[must_use]
pub fn site_of_url(url: &url::Url) -> String {
    match url.host_str() {
        Some(host) => {
            let site_host = silksurf_core::psl::registrable_domain(host)
                .unwrap_or_else(|| host.to_ascii_lowercase());
            format!("{}://{}", url.scheme(), site_host)
        }
        None => "null".to_string(),
    }
}

/// The live cookie partition key for a resource loaded under a top-level site.
///
/// An empty `top_level_site` (unknown / not plumbed) returns [`UNPARTITIONED`]
/// so cookies degrade to a single unpartitioned store. Otherwise the key is
/// `"<resource_site>^<top_level_site>"`, matching
/// `silksurf_engine::privacy::partition_key`'s format so a resource embedded
/// under two different top-level sites gets two isolated stores.
#[must_use]
pub fn partition_key(top_level_site: &str, resource_site: &str) -> String {
    if top_level_site.is_empty() {
        UNPARTITIONED.to_string()
    } else {
        format!("{resource_site}^{top_level_site}")
    }
}

/// The SameSite posture of a subresource request to `resource_site` under
/// `top_level_site`. An empty top-level site is [`SameSiteContext::Unknown`]
/// (no enforcement -- the degradation for an unplumbed path); a resource on the
/// top-level site is same-site; anything else is a cross-site subresource.
///
/// Top-level navigations are NOT classified here -- a subresource's cross-site
/// posture is destination-vs-top-level-site. A top-level navigation is
/// classified by [`navigation_same_site_context`] instead, from its initiator
/// site (which this function has no access to).
#[must_use]
pub fn subresource_same_site_context(top_level_site: &str, resource_site: &str) -> SameSiteContext {
    if top_level_site.is_empty() {
        SameSiteContext::Unknown
    } else if resource_site == top_level_site {
        SameSiteContext::SameSite
    } else {
        SameSiteContext::CrossSiteSubresource
    }
}

/// The SameSite posture of a top-level navigation to `destination_site`,
/// classified by the site that initiated it.
///
/// `initiator_site` is `None` for a browser-initiated navigation (address bar,
/// bookmark, history, the initial load): there is no cross-site initiator, so
/// it is same-site and every cookie -- including `Strict` -- is eligible. A
/// `Some(site)` initiator equal to the destination is likewise same-site.
///
/// A cross-site initiator (`Some(other)`) produces:
/// - [`SameSiteContext::CrossSiteTopLevel`] for a safe method (GET/HEAD):
///   `Strict` is withheld, `Lax` and `None` ride the navigation (this is what
///   keeps a normal cross-site link click logged in).
/// - [`SameSiteContext::CrossSiteSubresource`] for an unsafe method (POST etc.):
///   `Lax` does NOT ride an unsafe cross-site navigation (RFC 6265bis), so only
///   `None` is eligible -- the same filter a cross-site subresource uses, reused
///   here for a top-level cross-site POST.
///
/// An empty `destination_site` degrades to [`SameSiteContext::Unknown`] (no
/// enforcement), consistent with the unplumbed-path fallback elsewhere.
#[must_use]
pub fn navigation_same_site_context(
    initiator_site: Option<&str>,
    destination_site: &str,
    safe_method: bool,
) -> SameSiteContext {
    if destination_site.is_empty() {
        return SameSiteContext::Unknown;
    }
    match initiator_site {
        None => SameSiteContext::SameSite,
        Some(initiator) if initiator == destination_site => SameSiteContext::SameSite,
        Some(_) if safe_method => SameSiteContext::CrossSiteTopLevel,
        Some(_) => SameSiteContext::CrossSiteSubresource,
    }
}

/// Whether an HTTP method is "safe" for SameSite: GET and HEAD do not change
/// server state, so `Lax` cookies ride a cross-site top-level navigation using
/// them; every other method is unsafe and withholds `Lax`.
#[must_use]
pub fn is_safe_method(method: &str) -> bool {
    method.eq_ignore_ascii_case("get") || method.eq_ignore_ascii_case("head")
}

/// The SameSite attribute controlling cross-site sending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SameSite {
    /// Never sent on cross-site requests.
    Strict,
    /// Sent on top-level cross-site navigations only. The HTML default.
    #[default]
    Lax,
    /// Sent on all requests (requires Secure per modern rules).
    None,
}

/// The cross-site posture of a request, used to enforce SameSite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SameSiteContext {
    /// Same-site request: every cookie is eligible.
    SameSite,
    /// Cross-site top-level navigation: Strict cookies are withheld.
    CrossSiteTopLevel,
    /// Cross-site subresource: Strict and Lax cookies are withheld.
    CrossSiteSubresource,
    /// The top-level site is not plumbed yet, so SameSite is not enforced.
    Unknown,
}

/// One stored cookie.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    /// Effective domain (lowercased, no leading dot). For a host-only cookie
    /// this is the request host; matching then requires an exact host equal.
    pub domain: String,
    /// True when the cookie had no `Domain` attribute: it matches only its
    /// exact origin host, not subdomains.
    pub host_only: bool,
    pub path: String,
    /// Absolute expiry in Unix seconds; `None` is a session cookie.
    pub expires: Option<u64>,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: SameSite,
}

impl Cookie {
    #[must_use]
    pub fn is_expired(&self, now_unix: u64) -> bool {
        self.expires.is_some_and(|expiry| expiry <= now_unix)
    }

    /// RFC 6265 domain match: the request host equals the cookie domain, or
    /// (for non-host-only cookies) is a subdomain of it.
    #[must_use]
    pub fn domain_matches(&self, host: &str) -> bool {
        let host = host.to_ascii_lowercase();
        if host == self.domain {
            return true;
        }
        if self.host_only {
            return false;
        }
        host.ends_with(&self.domain) && host[..host.len() - self.domain.len()].ends_with('.')
    }

    /// RFC 6265 path match: the request path is the cookie path, a prefix of it
    /// ending at a `/`, or the cookie path is `/`.
    #[must_use]
    pub fn path_matches(&self, request_path: &str) -> bool {
        if self.path == request_path {
            return true;
        }
        if !request_path.starts_with(&self.path) {
            return false;
        }
        self.path.ends_with('/') || request_path[self.path.len()..].starts_with('/')
    }
}

/// Parse one `Set-Cookie` header value into a `Cookie`, resolving relative
/// attributes against the request host. Returns `None` for a malformed pair or
/// an immediately-expiring cookie the caller should treat as a deletion (use
/// `CookieStore::set_from_set_cookie`, which handles deletion).
#[must_use]
pub fn parse_set_cookie(header: &str, request_host: &str, now_unix: u64) -> Option<Cookie> {
    let mut parts = header.split(';');
    let pair = parts.next()?.trim();
    let (name, value) = pair.split_once('=')?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let request_host = request_host.to_ascii_lowercase();

    let mut cookie = Cookie {
        name: name.to_string(),
        value: value.trim().to_string(),
        domain: request_host.clone(),
        host_only: true,
        path: "/".to_string(),
        expires: None,
        secure: false,
        http_only: false,
        same_site: SameSite::Lax,
    };
    let mut max_age: Option<i64> = None;
    let mut expires: Option<u64> = None;

    for attr in parts {
        let attr = attr.trim();
        let (key, val) = match attr.split_once('=') {
            Some((k, v)) => (k.trim().to_ascii_lowercase(), v.trim().to_string()),
            None => (attr.to_ascii_lowercase(), String::new()),
        };
        match key.as_str() {
            "domain" if !val.is_empty() => {
                let domain = val.trim_start_matches('.').to_ascii_lowercase();
                if !domain.is_empty() {
                    cookie.domain = domain;
                    cookie.host_only = false;
                }
            }
            "path" if val.starts_with('/') => cookie.path = val,
            "max-age" => max_age = val.parse::<i64>().ok(),
            "expires" => expires = parse_http_date(&val),
            "secure" => cookie.secure = true,
            "httponly" => cookie.http_only = true,
            "samesite" => {
                cookie.same_site = match val.to_ascii_lowercase().as_str() {
                    "strict" => SameSite::Strict,
                    "none" => SameSite::None,
                    _ => SameSite::Lax,
                }
            }
            _ => {}
        }
    }

    // Max-Age takes precedence over Expires (RFC 6265 5.3).
    cookie.expires = match max_age {
        Some(seconds) if seconds <= 0 => Some(0),
        Some(seconds) => Some(now_unix.saturating_add(seconds as u64)),
        None => expires,
    };
    Some(cookie)
}

/// A set of stored cookies. Used both as a per-partition network jar and as a
/// document's `document.cookie` jar.
#[derive(Debug, Default, Clone)]
pub struct CookieStore {
    cookies: Vec<Cookie>,
}

impl CookieStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.cookies.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cookies.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Cookie> {
        self.cookies.iter()
    }

    /// Insert or replace a cookie. Identity is (name, domain, path) per RFC 6265.
    pub fn set(&mut self, cookie: Cookie) {
        if let Some(existing) = self.find_index(&cookie.name, &cookie.domain, &cookie.path) {
            self.cookies[existing] = cookie;
        } else {
            self.cookies.push(cookie);
        }
    }

    /// Apply a `Set-Cookie` header. An expired result deletes the matching
    /// cookie; a live result is stored. Returns true when the store changed.
    pub fn set_from_set_cookie(&mut self, header: &str, request_host: &str, now_unix: u64) -> bool {
        let Some(cookie) = parse_set_cookie(header, request_host, now_unix) else {
            return false;
        };
        if cookie.is_expired(now_unix) {
            return self
                .remove(&cookie.name, &cookie.domain, &cookie.path)
                .is_some();
        }
        self.set(cookie);
        true
    }

    /// Remove a cookie by identity, returning it if present.
    pub fn remove(&mut self, name: &str, domain: &str, path: &str) -> Option<Cookie> {
        self.find_index(name, domain, path)
            .map(|index| self.cookies.remove(index))
    }

    /// Drop every cookie whose expiry has passed.
    pub fn purge_expired(&mut self, now_unix: u64) {
        self.cookies.retain(|cookie| !cookie.is_expired(now_unix));
    }

    /// Build the `Cookie` request header for `host`/`path`/`secure` transport.
    /// `include_http_only` is true for network requests and false for
    /// `document.cookie`. `same_site_context` gates SameSite filtering;
    /// `Unknown` applies none (top-level site not yet plumbed).
    #[must_use]
    pub fn cookie_header(
        &self,
        host: &str,
        path: &str,
        secure: bool,
        include_http_only: bool,
        same_site_context: SameSiteContext,
        now_unix: u64,
    ) -> String {
        let mut matched: Vec<&Cookie> = self
            .cookies
            .iter()
            .filter(|cookie| !cookie.is_expired(now_unix))
            .filter(|cookie| cookie.domain_matches(host) && cookie.path_matches(path))
            .filter(|cookie| !cookie.secure || secure)
            .filter(|cookie| include_http_only || !cookie.http_only)
            .filter(|cookie| same_site_allows(cookie.same_site, same_site_context))
            .collect();
        // Longer paths sort first (RFC 6265 5.4 ordering by path length).
        matched.sort_by(|a, b| b.path.len().cmp(&a.path.len()));
        matched
            .iter()
            .map(|cookie| format!("{}={}", cookie.name, cookie.value))
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// Apply a `document.cookie` assignment scoped to the document `host`. Per
    /// the HTML spec a script cannot set an HttpOnly cookie, so such
    /// assignments are dropped. The cookie is parsed against `host` (so it is
    /// host-scoped unless it carries an explicit `Domain`), which lets it match
    /// HTTP requests to the same host in a shared jar. An empty `host` falls
    /// back to a synthetic jar host for a document with no origin (the `new()`
    /// stub context and unit tests).
    pub fn set_document_cookie(&mut self, assignment: &str, host: &str, now_unix: u64) {
        let script_sets_http_only = assignment
            .split(';')
            .skip(1)
            .any(|attr| attr.trim().eq_ignore_ascii_case("httponly"));
        if script_sets_http_only {
            return;
        }
        let effective_host = if host.is_empty() {
            DOCUMENT_JAR_HOST
        } else {
            host
        };
        let Some(mut cookie) = parse_set_cookie(assignment, effective_host, now_unix) else {
            return;
        };
        cookie.http_only = false;
        if cookie.is_expired(now_unix) {
            self.remove(&cookie.name, &cookie.domain, &cookie.path);
        } else {
            self.set(cookie);
        }
    }

    /// The `document.cookie` string for a document at `host`: `name=value`
    /// pairs for every non-expired, non-HttpOnly cookie whose domain matches
    /// `host`, in insertion order. An empty `host` matches every cookie (the
    /// `new()` stub context and unit tests, which have no document origin).
    /// Path is not matched -- document.cookie reads cover the origin's cookies.
    #[must_use]
    pub fn document_cookie_string(&self, host: &str, now_unix: u64) -> String {
        self.cookies
            .iter()
            .filter(|cookie| !cookie.is_expired(now_unix) && !cookie.http_only)
            .filter(|cookie| host.is_empty() || cookie.domain_matches(host))
            .map(|cookie| format!("{}={}", cookie.name, cookie.value))
            .collect::<Vec<_>>()
            .join("; ")
    }

    fn find_index(&self, name: &str, domain: &str, path: &str) -> Option<usize> {
        self.cookies.iter().position(|cookie| {
            cookie.name == name && cookie.domain == domain && cookie.path == path
        })
    }
}

/// A set of `CookieStore`s keyed by partition string, giving storage
/// partitioning: a cookie written under partition A is invisible under
/// partition B. The partition key is opaque to this type; the engine derives
/// it from (resource site, top-level site) -- see `silksurf_engine::privacy`.
#[derive(Debug, Default, Clone)]
pub struct PartitionedCookieStore {
    partitions: std::collections::HashMap<String, CookieStore>,
}

impl PartitionedCookieStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The cookie store for a partition, creating an empty one on first use.
    pub fn store_mut(&mut self, partition_key: &str) -> &mut CookieStore {
        self.partitions
            .entry(partition_key.to_string())
            .or_default()
    }

    /// The cookie store for a partition, if it exists.
    #[must_use]
    pub fn store(&self, partition_key: &str) -> Option<&CookieStore> {
        self.partitions.get(partition_key)
    }

    /// Number of populated partitions.
    #[must_use]
    pub fn partition_count(&self) -> usize {
        self.partitions.len()
    }

    /// Drop expired cookies across every partition.
    pub fn purge_expired(&mut self, now_unix: u64) {
        for store in self.partitions.values_mut() {
            store.purge_expired(now_unix);
        }
        self.partitions.retain(|_, store| !store.is_empty());
    }

    /// The `document.cookie` string for the top-level document at `host`. The
    /// document reads its own first-party partition (`resource == top-level`),
    /// so this uses `partition_key(top_level_site, top_level_site)`. An empty
    /// `top_level_site` reads the unpartitioned store (the stub context).
    #[must_use]
    pub fn document_cookie_string(
        &self,
        top_level_site: &str,
        host: &str,
        now_unix: u64,
    ) -> String {
        let key = partition_key(top_level_site, top_level_site);
        self.store(&key).map_or_else(String::new, |store| {
            store.document_cookie_string(host, now_unix)
        })
    }

    /// Apply a `document.cookie` assignment to the top-level document's
    /// first-party partition, scoped to `host`.
    pub fn set_document_cookie(
        &mut self,
        top_level_site: &str,
        host: &str,
        assignment: &str,
        now_unix: u64,
    ) {
        let key = partition_key(top_level_site, top_level_site);
        self.store_mut(&key)
            .set_document_cookie(assignment, host, now_unix);
    }
}

/// Whether a cookie's SameSite value permits sending in the given context.
fn same_site_allows(same_site: SameSite, context: SameSiteContext) -> bool {
    match context {
        SameSiteContext::Unknown | SameSiteContext::SameSite => true,
        SameSiteContext::CrossSiteTopLevel => same_site != SameSite::Strict,
        SameSiteContext::CrossSiteSubresource => same_site == SameSite::None,
    }
}

/// Parse an HTTP-date (RFC 1123 form, `Wdy, DD Mon YYYY HH:MM:SS GMT`) into
/// Unix seconds. Returns `None` for unrecognized formats; the epoch sentinel
/// used to delete cookies (`Thu, 01 Jan 1970 00:00:00 GMT`) parses to 0.
#[must_use]
pub fn parse_http_date(text: &str) -> Option<u64> {
    // Split off the weekday prefix ("Wdy, ") if present.
    let rest = text.split_once(", ").map_or(text, |(_, r)| r).trim();
    let mut fields = rest.split_whitespace();
    let day: u64 = fields.next()?.parse().ok()?;
    let month = month_index(fields.next()?)?;
    let year: u64 = fields.next()?.parse().ok()?;
    let mut hms = fields.next()?.split(':');
    let hour: u64 = hms.next()?.parse().ok()?;
    let minute: u64 = hms.next()?.parse().ok()?;
    let second: u64 = hms.next()?.parse().ok()?;
    if year < 1970 || day == 0 || day > 31 || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let days = days_from_civil(year, month, day)?;
    Some(days * 86400 + hour * 3600 + minute * 60 + second)
}

fn month_index(name: &str) -> Option<u64> {
    let months = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let lower = name.to_ascii_lowercase();
    months
        .iter()
        .position(|m| lower.starts_with(m))
        .map(|index| index as u64 + 1)
}

/// Days since 1970-01-01 for a civil date, via Howard Hinnant's algorithm.
fn days_from_civil(year: u64, month: u64, day: u64) -> Option<u64> {
    if !(1..=12).contains(&month) {
        return None;
    }
    let y = if month <= 2 { year - 1 } else { year };
    let era = y / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    // Days from 0000-03-01; shift to the 1970-01-01 epoch (719_468 days).
    Some(era * 146_097 + doe - 719_468)
}

/// Current time in Unix seconds, or 0 if the clock predates the epoch.
#[must_use]
pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_pair_defaults_to_host_only_session_cookie() {
        let cookie = parse_set_cookie("sid=abc123", "example.com", 1000).expect("parses");
        assert_eq!(cookie.name, "sid");
        assert_eq!(cookie.value, "abc123");
        assert_eq!(cookie.domain, "example.com");
        assert!(cookie.host_only);
        assert_eq!(cookie.path, "/");
        assert_eq!(cookie.expires, None);
        assert!(!cookie.secure && !cookie.http_only);
        assert_eq!(cookie.same_site, SameSite::Lax);
    }

    #[test]
    fn parse_attributes() {
        let cookie = parse_set_cookie(
            "id=7; Domain=.example.com; Path=/app; Secure; HttpOnly; SameSite=Strict; Max-Age=60",
            "www.example.com",
            1000,
        )
        .expect("parses");
        assert_eq!(cookie.domain, "example.com");
        assert!(!cookie.host_only);
        assert_eq!(cookie.path, "/app");
        assert!(cookie.secure && cookie.http_only);
        assert_eq!(cookie.same_site, SameSite::Strict);
        assert_eq!(cookie.expires, Some(1060));
    }

    #[test]
    fn max_age_overrides_expires_and_zero_deletes() {
        let cookie = parse_set_cookie(
            "x=1; Expires=Fri, 01 Jan 2100 00:00:00 GMT; Max-Age=0",
            "h",
            500,
        )
        .expect("parses");
        assert_eq!(cookie.expires, Some(0));
        assert!(cookie.is_expired(500));
    }

    #[test]
    fn http_date_parses_and_epoch_is_zero() {
        assert_eq!(parse_http_date("Thu, 01 Jan 1970 00:00:00 GMT"), Some(0));
        // 2021-01-01 00:00:00 UTC = 1609459200.
        assert_eq!(
            parse_http_date("Fri, 01 Jan 2021 00:00:00 GMT"),
            Some(1_609_459_200)
        );
        assert_eq!(parse_http_date("not a date"), None);
    }

    #[test]
    fn domain_and_path_matching() {
        let host_only = parse_set_cookie("a=1", "www.example.com", 0).unwrap();
        assert!(host_only.domain_matches("www.example.com"));
        assert!(!host_only.domain_matches("example.com"));
        assert!(!host_only.domain_matches("evil.com"));

        let domain_cookie =
            parse_set_cookie("a=1; Domain=example.com", "www.example.com", 0).unwrap();
        assert!(domain_cookie.domain_matches("www.example.com"));
        assert!(domain_cookie.domain_matches("example.com"));
        assert!(!domain_cookie.domain_matches("notexample.com"));

        let path_cookie = parse_set_cookie("a=1; Path=/app", "h", 0).unwrap();
        assert!(path_cookie.path_matches("/app"));
        assert!(path_cookie.path_matches("/app/page"));
        assert!(!path_cookie.path_matches("/apple"));
        assert!(!path_cookie.path_matches("/other"));
    }

    #[test]
    fn store_set_replaces_by_identity_and_purges_expired() {
        let mut store = CookieStore::new();
        store.set_from_set_cookie("a=1", "example.com", 0);
        store.set_from_set_cookie("a=2", "example.com", 0);
        assert_eq!(store.len(), 1);
        assert_eq!(store.iter().next().unwrap().value, "2");

        store.set_from_set_cookie("t=1; Max-Age=10", "example.com", 100);
        assert_eq!(store.len(), 2);
        store.purge_expired(115);
        assert_eq!(store.len(), 1, "expired temp cookie purged");
    }

    #[test]
    fn set_cookie_with_past_expiry_deletes() {
        let mut store = CookieStore::new();
        store.set_from_set_cookie("a=1", "example.com", 0);
        assert_eq!(store.len(), 1);
        let changed = store.set_from_set_cookie(
            "a=; Expires=Thu, 01 Jan 1970 00:00:00 GMT",
            "example.com",
            100,
        );
        assert!(changed);
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn cookie_header_filters_by_transport_and_flags() {
        let mut store = CookieStore::new();
        store.set_from_set_cookie("plain=1", "example.com", 0);
        store.set_from_set_cookie("secure=1; Secure", "example.com", 0);
        store.set_from_set_cookie("http=1; HttpOnly", "example.com", 0);

        // Insecure transport hides the Secure cookie; document.cookie hides HttpOnly.
        let doc = store.cookie_header(
            "example.com",
            "/",
            false,
            false,
            SameSiteContext::Unknown,
            0,
        );
        assert!(doc.contains("plain=1"));
        assert!(!doc.contains("secure=1"));
        assert!(!doc.contains("http=1"));

        // Secure network request sees all three.
        let net = store.cookie_header("example.com", "/", true, true, SameSiteContext::Unknown, 0);
        assert!(net.contains("plain=1") && net.contains("secure=1") && net.contains("http=1"));
    }

    #[test]
    fn document_cookie_round_trips_and_hides_http_only() {
        let mut store = CookieStore::new();
        store.set_document_cookie("theme=dark", "", 0);
        store.set_document_cookie("sid=xyz; Path=/", "", 0);
        assert_eq!(store.document_cookie_string("", 0), "theme=dark; sid=xyz");

        // A script cannot set an HttpOnly cookie.
        store.set_document_cookie("secret=1; HttpOnly", "", 0);
        assert!(!store.document_cookie_string("", 0).contains("secret"));

        // Expiry hides the cookie; a past expiry deletes it.
        store.set_document_cookie("temp=1; Max-Age=10", "", 100);
        assert!(store.document_cookie_string("", 105).contains("temp=1"));
        assert!(!store.document_cookie_string("", 115).contains("temp=1"));
        store.set_document_cookie("theme=; Max-Age=0", "", 200);
        assert!(!store.document_cookie_string("", 200).contains("theme"));
    }

    #[test]
    fn site_and_partition_key_derivation() {
        // The host is reduced to its registrable domain: www.example.com -> the
        // site https://example.com (port-independent).
        let url = url::Url::parse("https://www.example.com:8443/path").unwrap();
        assert_eq!(site_of_url(&url), "https://example.com");
        assert_eq!(
            partition_key("https://a.test", "https://res.test"),
            "https://res.test^https://a.test"
        );
        // Empty top-level site degrades to the unpartitioned store.
        assert_eq!(partition_key("", "https://res.test"), UNPARTITIONED);
    }

    #[test]
    fn navigation_context_classifies_by_initiator_and_method() {
        let dest = "https://bank.example";
        // Browser-initiated (address bar / bookmark / initial load): same-site.
        assert_eq!(
            navigation_same_site_context(None, dest, true),
            SameSiteContext::SameSite
        );
        // Same-site initiator: same-site regardless of method.
        assert_eq!(
            navigation_same_site_context(Some(dest), dest, false),
            SameSiteContext::SameSite
        );
        // Cross-site safe method (GET link click): Strict withheld, Lax rides.
        assert_eq!(
            navigation_same_site_context(Some("https://evil.example"), dest, true),
            SameSiteContext::CrossSiteTopLevel
        );
        // Cross-site unsafe method (POST): Lax also withheld (subresource-equal).
        assert_eq!(
            navigation_same_site_context(Some("https://evil.example"), dest, false),
            SameSiteContext::CrossSiteSubresource
        );
        // Empty destination degrades to no enforcement.
        assert_eq!(
            navigation_same_site_context(Some("https://evil.example"), "", true),
            SameSiteContext::Unknown
        );
        assert!(is_safe_method("GET") && is_safe_method("head") && !is_safe_method("POST"));
    }

    #[test]
    fn same_site_context_classifies_subresources() {
        let top = "https://example.com";
        assert_eq!(
            subresource_same_site_context(top, "https://example.com"),
            SameSiteContext::SameSite
        );
        assert_eq!(
            subresource_same_site_context(top, "https://tracker.test"),
            SameSiteContext::CrossSiteSubresource
        );
        // Empty top-level site: no enforcement (graceful degradation).
        assert_eq!(
            subresource_same_site_context("", "https://tracker.test"),
            SameSiteContext::Unknown
        );
    }

    #[test]
    fn partitioned_document_cookie_uses_first_party_partition() {
        let mut store = PartitionedCookieStore::new();
        let top = "https://shop.example";
        store.set_document_cookie(top, "shop.example", "cart=3", 0);
        assert_eq!(
            store.document_cookie_string(top, "shop.example", 0),
            "cart=3"
        );
        // A different top-level site sees nothing (isolated first-party jar).
        assert_eq!(
            store.document_cookie_string("https://other.example", "other.example", 0),
            ""
        );
        // The first-party partition is exactly partition_key(top, top).
        assert!(store.store(&partition_key(top, top)).is_some());
    }

    #[test]
    fn partitioned_store_isolates_cookies_by_key() {
        let mut store = PartitionedCookieStore::new();
        store
            .store_mut("siteA^top")
            .set_from_set_cookie("id=A", "example.com", 0);
        store
            .store_mut("siteB^top")
            .set_from_set_cookie("id=B", "example.com", 0);
        assert_eq!(store.partition_count(), 2);
        assert_eq!(
            store
                .store("siteA^top")
                .unwrap()
                .document_cookie_string("", 0),
            "id=A"
        );
        assert_eq!(
            store
                .store("siteB^top")
                .unwrap()
                .document_cookie_string("", 0),
            "id=B"
        );
        // A partition never observes another partition's cookies.
        assert!(store.store("siteA^top").unwrap().len() == 1);
    }

    #[test]
    fn same_site_filtering_respects_context() {
        let mut store = CookieStore::new();
        store.set_from_set_cookie("strict=1; SameSite=Strict", "example.com", 0);
        store.set_from_set_cookie("lax=1; SameSite=Lax", "example.com", 0);
        store.set_from_set_cookie("none=1; SameSite=None", "example.com", 0);

        let cross_sub = store.cookie_header(
            "example.com",
            "/",
            true,
            true,
            SameSiteContext::CrossSiteSubresource,
            0,
        );
        assert_eq!(cross_sub, "none=1");

        let cross_top = store.cookie_header(
            "example.com",
            "/",
            true,
            true,
            SameSiteContext::CrossSiteTopLevel,
            0,
        );
        assert!(cross_top.contains("lax=1") && cross_top.contains("none=1"));
        assert!(!cross_top.contains("strict=1"));

        // Unknown (top-level site not plumbed) applies no SameSite filtering.
        let unknown =
            store.cookie_header("example.com", "/", true, true, SameSiteContext::Unknown, 0);
        assert!(unknown.contains("strict=1"));
    }
}
