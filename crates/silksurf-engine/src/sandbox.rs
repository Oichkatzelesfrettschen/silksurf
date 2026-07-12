//! Origin and site classification for storage partitioning.
//!
//! This module answers same-origin / same-site questions: the classification a
//! browser uses to derive storage partition keys and cookie scoping. It does
//! NOT enforce isolation. Process-level site isolation -- separate renderer
//! processes, IPC, and OS sandboxing (seccomp/Landlock) -- is deferred to a
//! future process-model ADR per AD-022. Classification here is real;
//! enforcement is documented-absent, so the module cannot give a false
//! assurance that isolation exists.
//!
//! "Site" is scheme plus the registrable domain (eTLD+1), derived through the
//! Public Suffix List in `silksurf_core::psl`. So `a.example.com` and
//! `b.example.com` share a site, while `a.co.uk` and `b.co.uk` do not; a host
//! with no registrable domain (an IP literal, a bare public suffix,
//! `localhost`) is its own site. `site()` and `silksurf_net::cookie::
//! site_of_url` share that one derivation, so the classification here and the
//! live cookie keying agree.

/// A web origin. A tuple origin has a scheme, host, and optional port; an
/// opaque origin (serialized `null`) is same-origin only with itself.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Origin {
    Tuple {
        scheme: String,
        host: String,
        port: Option<u16>,
    },
    Opaque,
}

impl Origin {
    /// Parse a serialized origin (`scheme://host[:port]`). `null`, the empty
    /// string, and anything without a scheme separator are opaque.
    #[must_use]
    pub fn parse(origin: &str) -> Self {
        let origin = origin.trim();
        if origin.is_empty() || origin.eq_ignore_ascii_case("null") {
            return Origin::Opaque;
        }
        let Some((scheme, rest)) = origin.split_once("://") else {
            return Origin::Opaque;
        };
        if scheme.is_empty() {
            return Origin::Opaque;
        }
        // Authority ends at the first '/', '?', or '#'.
        let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
        // Strip userinfo if present.
        let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        let (host, port) = match host_port.rsplit_once(':') {
            Some((host, port_text)) => (host, port_text.parse::<u16>().ok()),
            None => (host_port, None),
        };
        if host.is_empty() {
            return Origin::Opaque;
        }
        Origin::Tuple {
            scheme: scheme.to_ascii_lowercase(),
            host: host.to_ascii_lowercase(),
            port,
        }
    }

    /// Serialize back to `scheme://host[:port]` or `null`.
    #[must_use]
    pub fn serialize(&self) -> String {
        match self {
            Origin::Opaque => "null".to_string(),
            Origin::Tuple { scheme, host, port } => match port {
                Some(port) => format!("{scheme}://{host}:{port}"),
                None => format!("{scheme}://{host}"),
            },
        }
    }

    /// Same-origin per the HTML spec: identical scheme, host, and port. Two
    /// opaque origins do not match -- without stable identity here, opaque is
    /// treated as maximally isolated.
    #[must_use]
    pub fn same_origin(&self, other: &Origin) -> bool {
        matches!((self, other), (Origin::Tuple { .. }, Origin::Tuple { .. })) && self == other
    }

    /// The site key: `scheme://registrable-domain` (port-independent), or `null`
    /// for opaque. The host is reduced to its eTLD+1 via the Public Suffix List;
    /// a host with no registrable domain keeps its full host.
    #[must_use]
    pub fn site(&self) -> String {
        match self {
            Origin::Opaque => "null".to_string(),
            Origin::Tuple { scheme, host, .. } => {
                let site_host =
                    silksurf_core::psl::registrable_domain(host).unwrap_or_else(|| host.clone());
                format!("{scheme}://{site_host}")
            }
        }
    }

    /// Same-site: identical site keys, and neither origin opaque.
    #[must_use]
    pub fn same_site(&self, other: &Origin) -> bool {
        matches!((self, other), (Origin::Tuple { .. }, Origin::Tuple { .. }))
            && self.site() == other.site()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_serialize_round_trip() {
        let origin = Origin::parse("https://www.example.com:8443/path?q=1");
        assert_eq!(
            origin,
            Origin::Tuple {
                scheme: "https".to_string(),
                host: "www.example.com".to_string(),
                port: Some(8443),
            }
        );
        assert_eq!(origin.serialize(), "https://www.example.com:8443");
        assert_eq!(
            Origin::parse("http://example.com").serialize(),
            "http://example.com"
        );
    }

    #[test]
    fn opaque_origins_for_null_and_malformed() {
        assert_eq!(Origin::parse("null"), Origin::Opaque);
        assert_eq!(Origin::parse(""), Origin::Opaque);
        assert_eq!(Origin::parse("not-an-origin"), Origin::Opaque);
        assert_eq!(Origin::parse("null").serialize(), "null");
    }

    #[test]
    fn same_origin_requires_scheme_host_port() {
        let a = Origin::parse("https://example.com");
        let b = Origin::parse("https://example.com");
        let http = Origin::parse("http://example.com");
        let ported = Origin::parse("https://example.com:8443");
        assert!(a.same_origin(&b));
        assert!(!a.same_origin(&http), "scheme differs");
        assert!(!a.same_origin(&ported), "port differs");
        // Opaque origins never match, even each other.
        assert!(!Origin::Opaque.same_origin(&Origin::Opaque));
    }

    #[test]
    fn same_site_is_port_independent_but_scheme_and_host_sensitive() {
        let a = Origin::parse("https://example.com:443");
        let b = Origin::parse("https://example.com:8443");
        let sub = Origin::parse("https://sub.example.com");
        let http = Origin::parse("http://example.com");
        assert!(a.same_site(&b), "port does not affect site");
        // Subdomains share the registrable domain, so they ARE same-site.
        assert!(
            a.same_site(&sub),
            "sub.example.com and example.com share a site"
        );
        assert!(!a.same_site(&http), "scheme differs");
        assert_eq!(a.site(), "https://example.com");
    }

    #[test]
    fn public_suffix_separates_registrable_domains() {
        // Under a multi-label public suffix, distinct registrable domains stay
        // distinct sites; a bare public suffix and an IP keep their full host.
        let a = Origin::parse("https://a.co.uk");
        let b = Origin::parse("https://b.co.uk");
        assert_eq!(a.site(), "https://a.co.uk");
        assert!(!a.same_site(&b), "a.co.uk and b.co.uk are different sites");
        assert_eq!(
            Origin::parse("https://shop.a.co.uk").site(),
            "https://a.co.uk"
        );
        assert_eq!(
            Origin::parse("https://127.0.0.1").site(),
            "https://127.0.0.1"
        );
    }
}
