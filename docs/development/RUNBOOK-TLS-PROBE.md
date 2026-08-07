# Runbook -- tls-probe

> `tls-probe` is the diagnostic binary for TLS handshake failures. When
> `silksurf-app` cannot reach a host, run `tls-probe` first. It produces a
> root-store inventory, performs a full handshake (with leaf-cert chain
> display), runs an OCSP / DANE probe, and emits an explicit RCA for the
> four canonical failure classes silksurf has seen in the wild.

The binary lives at `crates/silksurf-app/src/bin/tls_probe.rs`.
`crates/silksurf-tls` declares no binary target; loader coverage there runs
through `crates/silksurf-tls/tests/loader_sanity.rs`.

## Invocation

```sh
cargo run --release --bin tls-probe -- <hostname>
cargo run --release --bin tls-probe -- chatgpt.com
cargo run --release --bin tls-probe -- internal.corp --tls-ca-file /etc/ssl/corp.pem
cargo run --release --bin tls-probe -- example.com --platform-verifier
```

Flags:

  * `<hostname>` (positional, required) -- TLS server name to probe.
    Port defaults to 443.
  * `--tls-ca-file <path>` -- additional CA bundle in PEM form, appended
    to the platform root store. Used for corporate proxies and
    private-CA hosts.
  * `--platform-verifier` -- use `rustls-platform-verifier` instead of
    the bundled rustls + webpki-roots verifier.

## Output sections

  1. **Root store inventory.** Counts of native certs, webpki-roots
     constants, and any extra certs from `--tls-ca-file`. Also surfaces
     `SSL_CERT_FILE`, `SSL_CERT_DIR`, `NIX_SSL_CERT_FILE` env-var values
     for diagnostics.
  2. **TLS handshake.** Negotiated protocol version (expect TLS 1.3),
     cipher suite, ALPN result, and the peer leaf certificate chain
     printed in human-readable form (subject, issuer, validity window,
     SHA-256 fingerprint).
  3. **DANE TLSA probe.** DNSSEC-validated TLSA record fetch via
     hickory-resolver 0.26 with DNSSEC; reports the cert usage
     (PkixTa / PkixEe / DaneTa / DaneEe), selector, matching, and
     fingerprint hash.
  4. **RCA.** Explicit hint paragraph for the four canonical
     UnknownIssuer failure classes (see below).

## Canonical failure classes

| Symptom | RCA hint | Fix |
|---------|----------|-----|
| `0` native certs in root store | Nix env without `nixpkgs.cacert` activated; `SSL_CERT_FILE` unset | `nix-env -iA nixpkgs.cacert` or `export SSL_CERT_FILE=...` |
| Handshake fails on Cloudflare-fronted hosts; OpenSSL agrees with err 20 | Server presents an incomplete cert chain (missing intermediate) | Use `--tls-ca-file` with the missing intermediate, or rely on `rustls`'s `--platform-verifier` mode which can fetch via AIA |
| Handshake fails only on corporate networks; certificate issuer is unfamiliar | Corporate proxy CA injection | `--tls-ca-file /etc/ssl/corp.pem` (or whatever the corp policy file is) |
| `BadEncoding` on the TLSA fetch with `localdomain` appended to the FQDN | hickory-resolver 0.26 needs an explicit trailing dot on TLSA queries to prevent `/etc/resolv.conf` search-domain appending | Already fixed in `tls-probe` source; if you see this externally, ensure your FQDN ends in `.` |

## Output schema (planned)

The current output is human-readable. A JSON schema is planned so downstream
tools can parse the result; it is catalogued in
docs/roadmaps/DEBT-RECONCILIATION-ROADMAP.md.

## In-crate smoke

```sh
cargo test -p silksurf-tls --test loader_sanity
```

`crates/silksurf-tls` declares no binary target. `root_store_diagnostics`
is covered by `crates/silksurf-tls/tests/loader_sanity.rs`, which exercises
empty PEM, malformed PEM, the default-host loader, and root-store
diagnostics without depending on the silksurf-app stack. The DANE probe and
the RCA paragraph live only in the silksurf-app binary above.
