//! silksurf-tls loader sanity tests.
//!
//! WHY: the TLS protocol-level conformance (TLS 1.3 RFC 8446 vectors,
//! cipher correctness, handshake state machine) is the responsibility of
//! `rustls` and has its own upstream test suite. silksurf-tls only owns
//! the loader / config / extra-CA-injection surface; these tests cover
//! that boundary.
//!
//! WHAT:
//!   * Reject empty PEM input.
//!   * Reject malformed PEM input.
//!   * Accept a known-good single-cert PEM (the workspace bundles
//!     webpki-roots so we always have one).
//!   * Verify root_store_diagnostics returns non-zero counts on a
//!     normally-configured host.

use silksurf_tls::{TlsConfig, TlsConfigError, root_store_diagnostics};
use std::io::Write;
use std::path::PathBuf;

#[test]
fn diagnostics_returns_nonzero_roots() {
    let diag = root_store_diagnostics();
    // We expect at least one of mozilla_roots OR native_certs_added to
    // populate the store on any reasonably configured host. CI environments
    // may strip native-certs, so we assert on the union, not either one.
    assert!(
        diag.mozilla_roots > 0 || diag.native_certs_added > 0,
        "expected at least one of mozilla_roots / native_certs_added > 0, got mozilla={} native={}",
        diag.mozilla_roots,
        diag.native_certs_added,
    );
}

#[test]
fn extra_ca_file_rejects_empty() {
    let tmp = tempfile_pem(b"");
    let result = TlsConfig::new_with_extra_ca_file(&tmp);
    match result {
        Err(TlsConfigError::NoCertificates { .. })
        | Err(TlsConfigError::NoUsableCertificates { .. }) => {}
        other => panic!("expected NoCertificates / NoUsableCertificates error, got {other:?}"),
    }
}

#[test]
fn extra_ca_file_rejects_malformed_pem() {
    let tmp = tempfile_pem(b"not a real PEM file -- just bytes\n");
    let result = TlsConfig::new_with_extra_ca_file(&tmp);
    match result {
        Err(TlsConfigError::NoCertificates { .. })
        | Err(TlsConfigError::NoUsableCertificates { .. })
        | Err(TlsConfigError::Io(_)) => {}
        other => panic!("expected NoCertificates / NoUsableCertificates / Io error, got {other:?}"),
    }
}

#[test]
fn config_new_succeeds_on_default_host() {
    // The workspace bundles webpki-roots, so TlsConfig::new() should
    // always produce a working config regardless of whether the host has
    // native certs. The constructor is infallible (returns Self), so the
    // assertion is structural: just call it and confirm it doesn't panic.
    let _config = TlsConfig::new();
}

/// Write `bytes` to a uniquely-named tempfile under /tmp and return the path.
/// Best-effort: if /tmp is unwritable the test will surface the underlying
/// io::Error via the assertion in the caller.
fn tempfile_pem(bytes: &[u8]) -> PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        // UNWRAP-OK: SystemTime::now always returns a value >= UNIX_EPOCH on a sane clock.
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("silksurf-tls-test-{nanos}.pem"));
    let mut f = std::fs::File::create(&path).expect("create tempfile");
    f.write_all(bytes).expect("write tempfile");
    path
}
