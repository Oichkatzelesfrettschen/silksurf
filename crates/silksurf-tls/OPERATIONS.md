# silksurf-tls Operations

## Root store loading order

`TlsConfig::new()` builds the root store as:

  1. `webpki-roots` (Mozilla CA bundle, statically linked).
  2. `rustls-native-certs` (system trust store).
  3. (when `new_with_extra_ca_file` was used) the user-supplied PEM
     bundle, parsed with `rustls-pemfile`.

If steps 1 and 2 produce zero usable certs (e.g. on a Nix host with
`SSL_CERT_FILE` unset), `TlsConfigError::NoUsableCertificates` is
returned. This surfaces as the "0 native certs" RCA in the `tls-probe`
output.

## Env vars

  * `SSL_CERT_FILE` -- path to a PEM bundle, honored by
    `rustls-native-certs`.
  * `SSL_CERT_DIR` -- directory of PEM files, honored by
    `rustls-native-certs`.
  * `NIX_SSL_CERT_FILE` -- Nix-specific override; surfaced by
    `RootStoreDiagnostics` for diagnostic purposes (informational --
    silksurf-tls itself does not consult this var; it is the Nix
    convention that `nixpkgs.cacert` writes its bundle path here for
    `OPENSSL_DIR`-style consumers).

## Cipher roster

Inherited from rustls 0.23 defaults:

  * TLS 1.3: AEAD-only (`TLS13_AES_256_GCM_SHA384`,
    `TLS13_AES_128_GCM_SHA256`, `TLS13_CHACHA20_POLY1305_SHA256`).
  * TLS 1.2: AEAD ciphers with ECDHE key exchange. RC4, 3DES, CBC
    modes are absent.

## Post-quantum readiness

ML-KEM hybrid (X25519MLKEM768) is on the rustls roadmap; silksurf-tls
will pick it up automatically when rustls exposes it as a default. No
silksurf-tls change required. Tracked in SNAZZY-WAFFLE roadmap as
`crypto-agility / PQ` debt stream.

## OCSP stapling

Not yet enforced. The roadmap P5.S4 work wires RFC 6066 vectors and
defines the policy.

## HSTS

Not yet enforced. The roadmap P5.S4 work wires RFC 6797 behavior into
`silksurf-net`.

## Runtime CA injection

```sh
silksurf-app --tls-ca-file /etc/ssl/corp.pem https://internal.corp
```

The path is parsed once at startup; reloading at runtime is not
supported (would require restarting the renderer).

## Diagnostic flow

Use `tls-probe` from `silksurf-app`:

```sh
cargo run --release --bin tls-probe -- example.com
```

See `docs/development/RUNBOOK-TLS-PROBE.md` for the full RCA flow.
