# Networking & TLS

This document describes SilkSurf's cleanroom networking layer and the TLS
certificate validation surface.

## Crates

- `crates/silksurf-net`: HTTP request/response types, `NetClient`, synchronous
  HTTP/1.1 fetch, and HTTP/2 fanout.
- `crates/silksurf-tls`: `rustls` configuration, root-store loading, insecure
  debug mode, and the `tls_probe` diagnostic binary.

## Current Behavior

- `BasicClient` uses `RustlsProvider` by default.
- `TlsConfig::new()` builds a `rustls::RootCertStore` from Mozilla roots via
  `webpki-roots` and native/system certificates via `rustls-native-certs`.
- HTTPS fetches now complete the rustls handshake before writing the HTTP
  request, so certificate failures are reported as `TLS handshake: ...` instead
  of being deferred until the first write.
- `TlsConfig::new_insecure()` disables certificate validation for diagnostics
  only; user-facing launches expose this through the app's `--insecure` path.

## Certificate Failure RCA

Failure observed on 2026-04-11 while launching the app:

```text
[SilkSurf] Fetching: https://example.com
[SilkSurf] Fetch error: TLS write: invalid peer certificate: UnknownIssuer
```

The error was originally labelled as `TLS write` because rustls performs the
client handshake lazily during I/O. The certificate validation failure itself
was still a handshake failure.

Independent system checks showed the same host-chain trust problem outside
SilkSurf:

```text
openssl s_client -connect example.com:443 -servername example.com -showcerts
verify error:num=20:unable to get local issuer certificate
Verify return code: 20 (unable to get local issuer certificate)

curl -Iv https://example.com
curl: (60) SSL certificate OpenSSL verify result: unable to get local issuer certificate
```

The server chain observed in that session was:

```text
leaf:   CN=example.com
issuer: C=US, O=CLOUDFLARE, INC., CN=Cloudflare TLS Issuing ECC CA 1

intermediate: C=US, O=CLOUDFLARE, INC., CN=Cloudflare TLS Issuing ECC CA 1
issuer:       C=US, O=SSL Corporation, CN=SSL.com TLS Transit ECC CA R2

intermediate: C=US, O=SSL Corporation, CN=SSL.com TLS Transit ECC CA R2
issuer:       C=GB, ST=Greater Manchester, L=Salford, O=Comodo CA Limited, CN=AAA Certificate Services
```

Conclusion: this is not currently isolated to SilkSurf. The host system's
OpenSSL/curl trust path also failed to build a trusted issuer chain for the
same target. SilkSurf's rustls stack is surfacing the same trust-anchor or
served-chain failure through `UnknownIssuer`.

## Diagnostic Command

Use the TLS probe to capture SilkSurf's root-store counts and force a TLS
handshake without sending an HTTP request:

```sh
cargo run -p silksurf-tls --bin tls_probe -- example.com 443
```

The probe prints:

- Mozilla/webpki root count.
- Native certs loaded, added to rustls, and rejected by rustls parsing.
- Relevant certificate environment variables:
  `SSL_CERT_FILE`, `SSL_CERT_DIR`, and `NIX_SSL_CERT_FILE`.
- Native cert loader errors, if any.
- Explicit rustls handshake result and negotiated ALPN, if the handshake
  succeeds.

Current local probe output for `example.com:443`:

```text
Mozilla/webpki roots: 136
Native certs: loaded=145, added=145, rejected=0
Total rustls roots: 281
Cert env: SSL_CERT_FILE=Some("/etc/ssl/cert.pem"), SSL_CERT_DIR=Some("/etc/ssl/certs"), NIX_SSL_CERT_FILE=Some("/etc/ssl/certs/ca-certificates.crt")
Native cert loader errors: none
TLS handshake: failed: rustls complete_io: invalid peer certificate: UnknownIssuer
```

## Crate Categories To Search

- Trust-store loading: `rustls-native-certs`, `webpki-roots`, `openssl-probe`.
- Platform verification: `rustls-platform-verifier`.
- TLS client stacks and adapters: `rustls`, `tokio-rustls`, `hyper-rustls`.
- X.509 and chain diagnostics: `x509-parser`, `cert-dump`, `tlsinspect`.
- Certificate Transparency or external observability, for diagnostics only:
  `sct`, `crt-sh`.
- Server-side certificate automation, generally out of scope for SilkSurf's
  client validation path: `rustls-acme`, ACME companion crates.

## Next Remediation Options

- Keep the current rustls WebPKI path and fix the host trust store when curl and
  OpenSSL fail the same target.
- Add `rustls-platform-verifier` behind a feature flag if SilkSurf needs browser
  or OS-native path-building semantics on platforms where WebPKI plus native
  roots is not enough.
- Add an offline regression test for diagnostic formatting and keep live network
  certificate validation in manual probes, because public certificate chains can
  change without a source-code change.

## Engine Integration

- `silksurf-engine` uses `NetClient` to fetch HTML/CSS/JS.
- Network responses feed the parser pipeline and JS task queue.
- Network caching lives in `crates/silksurf-net/src/cache.rs`.
