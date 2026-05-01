# silksurf-tls

TLS adapter layer over `rustls`. Loads root certs (Mozilla webpki-roots
+ system native-certs), exposes a configured `ClientConfig`, supplies
the optional platform verifier, and lets callers attach extra CAs at
runtime via `--tls-ca-file`.

## Public API

  * `TlsConfig` -- the configured `Arc<ClientConfig>` plus h2-specific
    config (ALPN advertised).
  * `TlsConfig::new()`, `new_h2()`, `new_with_extra_ca_file(path)`,
    `new_h2_with_extra_ca_file(path)`, `new_with_platform_verifier()`
    (feature-gated `platform-verifier`).
  * `TlsConfigError` -- crate-local error covering Io, Rustls,
    NoCertificates, NoUsableCertificates. `From<TlsConfigError> for
    silksurf_core::SilkError` at the bottom of `lib.rs`.
  * `RootStoreDiagnostics` + `root_store_diagnostics()` -- counts of
    native + webpki-roots + extra certs, plus the
    `SSL_CERT_FILE` / `SSL_CERT_DIR` / `NIX_SSL_CERT_FILE` env-var
    snapshot. Surfaced by `tls-probe`.

## Bins

  * `tls_probe` (in-crate smoke; suppressed from `cargo doc` to avoid
    collision with the canonical `silksurf-app/src/bin/tls_probe.rs`).
    See OPERATIONS.md for invocation conventions.

## Conventions

  * Defaults to TLS 1.3-only behavior when the platform verifier is
    used; otherwise inherits rustls 0.23's TLS 1.2/1.3 support.
  * Banned crates: `openssl`, `openssl-sys` (see `deny.toml`).
  * `rustls-pemfile` is currently used for PEM loading; migration to
    `rustls-pki-types::pem::PemObject` is a tracked follow-up
    (RUSTSEC-2025-0134; rustls-pemfile is unmaintained but not
    vulnerable).

## See Also

  * `OPERATIONS.md` for cipher roster and runtime behavior
  * `docs/NETWORK_TLS.md` for the broader TLS posture
  * `docs/development/RUNBOOK-TLS-PROBE.md` for handshake diagnosis
  * `docs/design/THREAT-MODEL.md` Subsystem 2
