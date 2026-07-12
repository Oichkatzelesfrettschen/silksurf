//! Public Suffix List matcher: registrable-domain (eTLD+1) derivation.
//!
//! A "site" for cookies, storage partitioning, and same-site comparison is
//! scheme plus the *registrable domain* -- the public suffix (effective TLD)
//! plus one more label. `foo.co.uk` and `bar.co.uk` are different sites because
//! `co.uk` is a public suffix; `a.example.com` and `b.example.com` share the
//! site `example.com`. Deriving that boundary correctly requires the Public
//! Suffix List (`https://publicsuffix.org`), since the eTLD is not computable
//! from the hostname alone (`com` is one label, `co.uk` is two, `*.ck` covers
//! any label under `ck`).
//!
//! `registrable_domain` is the single entry point both site derivations call
//! -- `silksurf_net::cookie::site_of_url` and `silksurf_engine::sandbox::
//! Origin::site` -- so the two agree on every boundary.
//!
//! Data provenance: `data/public_suffix_list.dat` is pulled verbatim from
//! `https://publicsuffix.org/list/public_suffix_list.dat` (the file's own header
//! records its VERSION and COMMIT). It is licensed MPL-2.0; that notice heads
//! the vendored file and is preserved. Both the ICANN and PRIVATE sections are
//! used: including PRIVATE separates e.g. `a.github.io` from `b.github.io`,
//! which over-separates rather than under-separates -- the safe direction for
//! isolation.
//!
//! Matching (per the publicsuffix.org algorithm): the prevailing rule is the
//! one matching the most labels; an exception rule (`!`) beats any other; when
//! nothing matches, the default rule `*` makes the rightmost label the public
//! suffix. U-label (Unicode) rules in the list are normalized to Punycode at
//! load, because `url::host_str` yields Punycode; skipping that would leave IDN
//! hosts matching only the default `*` rule and thus silently over-grouped.

use std::collections::HashSet;
use std::sync::OnceLock;

/// The vendored Public Suffix List, embedded at compile time.
const LIST: &str = include_str!("../data/public_suffix_list.dat");

/// Parsed rule sets. Rules are stored by kind so matching is a set lookup per
/// candidate suffix rather than a scan of the whole list.
struct PublicSuffixList {
    /// Normal rules, e.g. `com`, `co.uk` (already Punycode-normalized).
    rules: HashSet<String>,
    /// Wildcard bases: for a rule `*.ck` the base `ck` is stored; a candidate
    /// matches when its remaining labels (after the leftmost) equal a base.
    wildcards: HashSet<String>,
    /// Exception domains: for a rule `!www.ck` the domain `www.ck` is stored.
    exceptions: HashSet<String>,
}

/// Parse the list once, on first use.
fn list() -> &'static PublicSuffixList {
    static PSL: OnceLock<PublicSuffixList> = OnceLock::new();
    PSL.get_or_init(|| parse(LIST))
}

/// Normalize a rule (or host) to its A-label (Punycode, lowercase) form. Rules
/// that already are ASCII pass through `domain_to_ascii` unchanged; U-label
/// rules become the Punycode a `url` host string carries. A rule IDNA rejects
/// is dropped (it could never match a valid host anyway).
fn to_ascii(domain: &str) -> Option<String> {
    idna::domain_to_ascii(domain).ok()
}

fn parse(text: &str) -> PublicSuffixList {
    let mut rules = HashSet::new();
    let mut wildcards = HashSet::new();
    let mut exceptions = HashSet::new();
    for line in text.lines() {
        let line = line.trim();
        // A PSL line is either blank, a `//` comment, or one rule; rules carry
        // no inline comments and are read whole.
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        if let Some(exception) = line.strip_prefix('!') {
            if let Some(ascii) = to_ascii(exception) {
                exceptions.insert(ascii);
            }
        } else if let Some(base) = line.strip_prefix("*.") {
            if let Some(ascii) = to_ascii(base) {
                wildcards.insert(ascii);
            }
        } else if let Some(ascii) = to_ascii(line) {
            rules.insert(ascii);
        }
    }
    PublicSuffixList {
        rules,
        wildcards,
        exceptions,
    }
}

/// The registrable domain (eTLD+1) of `host`, or `None` when `host` has no
/// registrable domain and the caller should keep the full host as its own site.
///
/// `None` is returned for an IP literal (no eTLD), a single-label host
/// (`localhost`), and a host that is *itself* a public suffix (`co.uk`,
/// `github.io`) -- returning the bare suffix would collapse distinct sites, so
/// the maximally-partitioned full host is kept instead.
///
/// The result is lowercase A-label (Punycode) form, matching `url::host_str`.
#[must_use]
pub fn registrable_domain(host: &str) -> Option<String> {
    let host = host.trim_matches('.').to_ascii_lowercase();
    if host.is_empty() || host.parse::<std::net::IpAddr>().is_ok() {
        return None;
    }
    let labels: Vec<&str> = host.split('.').collect();
    let label_count = labels.len();
    if label_count < 2 {
        return None;
    }
    let psl = list();

    // Public suffix length in labels. An exception rule wins outright: its
    // public suffix is the exception domain minus its leftmost label.
    let mut suffix_labels = None;
    for start in 0..label_count {
        if psl.exceptions.contains(&labels[start..].join(".")) {
            suffix_labels = Some(label_count - (start + 1));
            break;
        }
    }
    // Otherwise the longest matching normal or wildcard rule prevails; iterating
    // from the leftmost start yields the longest candidate first.
    if suffix_labels.is_none() {
        for start in 0..label_count {
            let candidate = labels[start..].join(".");
            let wildcard_hit =
                start + 1 < label_count && psl.wildcards.contains(&labels[start + 1..].join("."));
            if psl.rules.contains(&candidate) || wildcard_hit {
                suffix_labels = Some(label_count - start);
                break;
            }
        }
    }
    // No rule matched: the default rule `*` makes the rightmost label the eTLD.
    let suffix_labels = suffix_labels.unwrap_or(1);

    // The registrable domain is the public suffix plus one more label. A host
    // that is exactly a public suffix has none.
    if label_count <= suffix_labels {
        return None;
    }
    Some(labels[label_count - suffix_labels - 1..].join("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_single_label_etlds() {
        assert_eq!(
            registrable_domain("www.example.com").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            registrable_domain("a.b.example.com").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            registrable_domain("example.com").as_deref(),
            Some("example.com")
        );
    }

    #[test]
    fn multi_label_etld_keeps_sites_distinct() {
        // co.uk is a public suffix, so a.co.uk and b.co.uk are DIFFERENT sites.
        assert_eq!(registrable_domain("a.co.uk").as_deref(), Some("a.co.uk"));
        assert_eq!(registrable_domain("b.co.uk").as_deref(), Some("b.co.uk"));
        assert_ne!(registrable_domain("a.co.uk"), registrable_domain("b.co.uk"));
        assert_eq!(
            registrable_domain("shop.a.co.uk").as_deref(),
            Some("a.co.uk")
        );
    }

    #[test]
    fn private_section_separates_hosted_subdomains() {
        // github.io is a PRIVATE-section rule: distinct user pages are distinct
        // sites, and bare github.io has no registrable domain.
        assert_eq!(
            registrable_domain("alice.github.io").as_deref(),
            Some("alice.github.io")
        );
        assert_ne!(
            registrable_domain("alice.github.io"),
            registrable_domain("bob.github.io")
        );
        assert_eq!(registrable_domain("github.io"), None);
    }

    #[test]
    fn wildcard_and_exception_rules() {
        // *.ck makes any label under ck a public suffix...
        assert_eq!(
            registrable_domain("foo.bar.ck").as_deref(),
            Some("foo.bar.ck")
        );
        // ...except !www.ck, which pins www.ck's suffix back to ck.
        assert_eq!(registrable_domain("www.ck").as_deref(), Some("www.ck"));
        assert_eq!(registrable_domain("site.www.ck").as_deref(), Some("www.ck"));
    }

    #[test]
    fn idn_etld_is_not_over_grouped() {
        // 公司.cn (xn--55qx5d.cn) is a U-label rule; a Punycode host under it
        // must resolve to <label>.xn--55qx5d.cn, not collapse to the cn default.
        assert_eq!(
            registrable_domain("shop.xn--55qx5d.cn").as_deref(),
            Some("shop.xn--55qx5d.cn")
        );
        assert_ne!(
            registrable_domain("a.xn--55qx5d.cn"),
            registrable_domain("b.xn--55qx5d.cn")
        );
    }

    #[test]
    fn hosts_without_registrable_domains() {
        assert_eq!(registrable_domain("127.0.0.1"), None);
        assert_eq!(registrable_domain("::1"), None);
        assert_eq!(registrable_domain("localhost"), None);
        assert_eq!(registrable_domain(""), None);
        // A host under an unknown TLD falls back to the default `*` rule.
        assert_eq!(
            registrable_domain("host.invalidtld").as_deref(),
            Some("host.invalidtld")
        );
    }

    #[test]
    fn trailing_dot_and_case_are_normalized() {
        assert_eq!(
            registrable_domain("WWW.Example.CoM.").as_deref(),
            Some("example.com")
        );
    }
}
