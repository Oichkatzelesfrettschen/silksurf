/*
 * IDN / Punycode round-trip tests.
 *
 * WHY: The `url` crate (already a direct dep of silksurf-net) delegates
 * internationalized hostname processing to the `idna` crate (v1.1.0,
 * transitive).  These tests verify that ACE-encoded (xn--) hostnames
 * survive a URL host parse without mangling and that the host field the
 * engine sees is the canonical ASCII-compatible encoding (ACE), not a
 * percent-encoded or otherwise corrupted form.
 *
 * HOW: Construct a URL string that contains a known Punycode label, parse
 * it with `url::Url::parse`, and assert that the `host_str()` returned by
 * the url crate matches the expected ACE form.  No network I/O occurs.
 *
 * LIMITATION NOTE: The url crate does not expose raw Punycode
 * encode/decode as a public API; the tests therefore assert on the
 * round-trip behaviour of full URL parsing rather than on the codec itself.
 * A standalone Punycode codec test would require adding the `idna` crate
 * as a direct dev-dependency; that is deferred per AD-021.
 */

/// "b\u{00fc}cher.example" (the German word "Bucher" with the umlaut on
/// the 'u') encoded as an ACE label: xn--bcher-kva.example.
///
/// Verified with Python encodings.idna.ToASCII and RFC 3492 Punycode codec.
const ACE_HOST: &str = "xn--bcher-kva.example";

/// The Unicode form of the above domain: "b\u{00fc}cher.example"
/// (U+00FC = LATIN SMALL LETTER U WITH DIAERESIS, i.e. u-umlaut).
/// The url crate's IDNA processing should encode this to `ACE_HOST` on parse.
const UNICODE_HOST: &str = "b\u{00fc}cher.example";

// --------------------------------------------------------------------------
// Tests
// --------------------------------------------------------------------------

/// Verify that a URL whose host is already ACE-encoded survives a parse
/// without mangling.  This is the primary round-trip assertion: whatever
/// host bytes were in the URL string must come back unchanged from
/// `host_str()`.
#[test]
fn ace_host_survives_url_parse() {
    let raw = format!("https://{ACE_HOST}/path?q=1#frag");
    let parsed = url::Url::parse(&raw).expect("valid URL with ACE host");
    let host = parsed.host_str().expect("URL has a host");
    assert_eq!(
        host, ACE_HOST,
        "ACE host must be returned verbatim by host_str()"
    );
}

/// Verify that the scheme, port, path, query, and fragment survive
/// alongside an ACE host so that no component bleeds into the host field.
#[test]
fn ace_host_components_are_clean() {
    let raw = format!("https://{ACE_HOST}:8443/a/b?x=y#z");
    let parsed = url::Url::parse(&raw).expect("valid URL");
    assert_eq!(parsed.scheme(), "https");
    assert_eq!(parsed.host_str().unwrap(), ACE_HOST);
    assert_eq!(parsed.port(), Some(8443));
    assert_eq!(parsed.path(), "/a/b");
    assert_eq!(parsed.query(), Some("x=y"));
    assert_eq!(parsed.fragment(), Some("z"));
}

/// Verify that a URL with a Unicode hostname is normalised to the ACE form
/// that the engine's network stack will actually use when opening
/// connections.  The url crate applies UTS#46 mapping and then Punycode-
/// encodes each non-ASCII label.
///
/// NOTE: this test documents that the url crate performs the encoding; it
/// is NOT testing a silksurf-net codec.  If the url crate's behaviour
/// changes, this test will catch the regression.
#[test]
fn unicode_host_is_normalised_to_ace() {
    let raw = format!("https://{UNICODE_HOST}/");
    let parsed = url::Url::parse(&raw).expect("valid URL with Unicode host");
    let host = parsed.host_str().expect("URL has a host");
    assert_eq!(
        host, ACE_HOST,
        "url crate must encode the Unicode host to ACE form"
    );
}

/// Verify that an already-correct ACE label on the loopback address does
/// not acquire spurious percent-encoding or other mangling.
#[test]
fn ace_host_no_percent_encoding_introduced() {
    let raw = format!("http://{ACE_HOST}/");
    let as_str = url::Url::parse(&raw).expect("valid URL").to_string();
    // The serialised URL must not contain a percent sign in the host.
    // (Path/query percent-encoding is fine; host encoding is not.)
    let host_part = as_str
        .strip_prefix("http://")
        .unwrap()
        .split('/')
        .next()
        .unwrap();
    assert!(
        !host_part.contains('%'),
        "host must not contain percent-encoded bytes; got: {host_part}"
    );
}

/// Document the current limitation: silksurf-net has no standalone Punycode
/// codec exposed at the crate boundary.  Encoding/decoding a bare label
/// (without a scheme or path) is not directly testable without the `idna`
/// crate as a direct dev-dependency, which AD-021 defers.
///
/// When that dep is added, replace this test with a codec round-trip:
///   `idna::domain_to_ascii("bucher\u{fc}.example`") == Ok("xn--bcher-kva.example")
///   idna::domain_to_unicode("xn--bcher-kva.example").0 == "bucheru.example" (approx)
#[test]
#[ignore = "AD-021 defers direct idna dep; replace with codec round-trip when idna is a direct dev-dep"]
fn standalone_punycode_codec_round_trip() {
    // This body is intentionally unreachable while the test is #[ignore].
    // It documents what the test SHOULD do when the dep lands.
    //
    // let encoded = idna::domain_to_ascii(UNICODE_HOST).unwrap();
    // assert_eq!(encoded, ACE_HOST);
    // let (decoded, errors) = idna::domain_to_unicode(&encoded);
    // assert!(errors.is_ok());
    // assert_eq!(decoded, UNICODE_HOST.to_lowercase());
    panic!("not yet implemented -- see AD-021");
}
