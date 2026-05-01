# Threat Model -- silksurf engine surface

> A web browser parses arbitrary bytes from the public internet, runs
> arbitrary scripts, mounts arbitrary cookies into HTTP requests, and
> renders arbitrary pixels. silksurf's threat surface is broad even at
> the v0.1 engine-only stage. This document catalogues what we defend
> against, what we explicitly do not yet defend against, and where the
> known gaps live.

This is a STRIDE-style pass. Each subsystem gets a row per relevant
category (Spoofing, Tampering, Repudiation, Information disclosure,
Denial of service, Elevation of privilege). Empty cells = not applicable
or not exposed at the v0.1 surface.

## Subsystem 1: Network (`silksurf-net`)

| STRIDE | Threat | Mitigation today | Gap |
|--------|--------|------------------|-----|
| Spoofing | DNS rebinding, IP spoofing | hickory-resolver with DNSSEC | No SNI pinning; revisit if a host enables HSTS preload list |
| Tampering | MitM on plaintext HTTP | `silksurf-net` defaults to HTTP/2 over TLS 1.3 | No HSTS enforcement yet |
| Repudiation | n/a (no audit log on requests) | -- | Logging is pending observability work (P8.S6) |
| Info disclosure | Cache directory contains response bodies (`Cache-Control: private` not enforced on disk) | `~/.cache/silksurf/http` is mode 0700 by default | Need disk-encryption-at-rest discipline; document in OPERATIONS |
| Denial of service | Large response bodies / stalled connections | tokio default timeouts + per-request limits TBD | No max-body-size cap yet -- tracked in P8.S8 |
| Elevation of priv | n/a -- network code does not exec | -- | -- |

## Subsystem 2: TLS (`silksurf-tls`)

| STRIDE | Threat | Mitigation today | Gap |
|--------|--------|------------------|-----|
| Spoofing | Forged server cert | rustls + webpki-roots; optional `rustls-platform-verifier`; `--tls-ca-file` for private CA | OCSP stapling is documented in `docs/NETWORK_TLS.md` but not yet enforced |
| Tampering | Downgrade to TLS 1.2 / SSL 3 | rustls is TLS 1.2/1.3 only; SSL 3 not present | Force TLS 1.3-only mode TBD |
| Info disclosure | Leak cipher state on side channel | rustls uses constant-time AES-GCM and ChaCha20-Poly1305 | No side-channel hardening claims beyond the rustls baseline |
| Denial of service | Handshake bombs | tokio-rustls timeout TBD | Per-handshake budget cap tracked in P8.S8 |

## Subsystem 3: HTML / CSS / DOM parsers (`silksurf-html`, `silksurf-css`, `silksurf-dom`)

| STRIDE | Threat | Mitigation today | Gap |
|--------|--------|------------------|-----|
| Tampering | Malformed input crashes the parser | All three are fuzzed (libfuzzer-sys + AFL++) | Seed corpus is stub-sized; expand in P3.S1 |
| Info disclosure | Selector with regex-like complexity exposes timing channel | No regex anywhere in the parser surface (verified by audit) | -- |
| Denial of service | Pathological CSS (10k selectors, deep nesting) blows up cascade | No max-rule-count cap | Tracked in P8.S8 |
| Denial of service | Pathological HTML (deep nesting, mismatched tags) blows up tree builder | TreeBuilder uses a state machine without unbounded recursion | But no max-depth cap; tracked in P8.S8 |

## Subsystem 4: Engine pipeline (`silksurf-engine`)

| STRIDE | Threat | Mitigation today | Gap |
|--------|--------|------------------|-----|
| Tampering | Cross-DOM atom confusion (forged `Atom` from another interner) | `Atom` is Copy and not signed; `resolve()` panics on out-of-range index | Documented invariant: never share Atoms across `Dom` instances. Add `#[doc(hidden)]` `dom_id` field if needed |
| Denial of service | Layout pass on pathological tree | No layout-pass timeout | Tracked in P8.S8 |

## Subsystem 5: JS runtime (`silksurf-js`)

| STRIDE | Threat | Mitigation today | Gap |
|--------|--------|------------------|-----|
| Tampering | Script escapes the bytecode VM | The VM is a custom interpreter with no JIT; no JIT-spray surface | Sandbox boundary not formally specified; treat as untrusted equivalent for v0.1 |
| Info disclosure | Side-channel timing of object lookups | Hidden classes and IC are not yet implemented | Cache-side-channel work TBD |
| Denial of service | Infinite loop / unbounded recursion / runaway alloc | No per-VM step budget; no max-stack-depth cap | Tracked in P8.S8 |
| Elevation of priv | FFI boundary panic via non-UTF-8 string | **KNOWN BUG** at `silksurf-js/src/ffi.rs:271` -- `unwrap()` inside `unsafe { CStr::from_ptr }.to_str()` panics across the FFI boundary | Tracked in the silksurf-js unsafe/unwrap follow-up batch |

## Subsystem 6: Render (`silksurf-render`)

| STRIDE | Threat | Mitigation today | Gap |
|--------|--------|------------------|-----|
| Tampering | Out-of-bounds blit on pathological display list | Per-tile bounds checks before each `slice::from_raw_parts_mut` (see `docs/design/UNSAFE-CONTRACTS.md` lines 270, 444) | -- |
| Denial of service | Megapixel rasterization | No render-target size cap | Tracked in P8.S8 |
| Info disclosure | Information leak via canvas readback | Canvas API not yet implemented | Treat as preventive: audit canvas before exposing |

## Subsystem 7: Persistent cache (`silksurf-net::cache`)

| STRIDE | Threat | Mitigation today | Gap |
|--------|--------|------------------|-----|
| Tampering | Path traversal on cache filename | Filename = `FxHash(url)` hex; no slashes possible | -- |
| Info disclosure | Cache contains private response bodies | Directory mode 0700 by default | No encryption at rest; Cache-Control: private not enforced |
| Repudiation | n/a | -- | -- |

## Cross-cutting -- not yet started (v0.2+ work)

  * **Site isolation.** No process-per-origin separation. JS from origin
    A can in principle observe DOM state from origin B if the engine is
    embedding multiple documents. v0.1 ships single-origin only.
  * **Third-party cookies / storage partitioning.** No cookie subsystem
    yet.
  * **Fingerprinting surface.** Canvas, WebGL, Audio, fonts, screen
    resolution -- none enumerated yet.
  * **Subresource Integrity (SRI).** Not yet validated.
  * **CORS.** Not yet enforced.
  * **CSP (Content-Security-Policy).** Not yet honored.

These are explicit future work; the v0.1 release notes will name them.

## Update cadence

This document should be revisited at every roadmap-wave boundary
(currently SNAZZY-WAFFLE wave 1-6). When a Subsystem row changes
(mitigation lands, new threat surfaces), bump the table and add a note
in the relevant ADR.

## Related

  * `/SECURITY.md` -- security policy and reporting
  * `/docs/design/UNSAFE-CONTRACTS.md` -- unsafe-block index
  * `/docs/design/ARCHITECTURE-DECISIONS.md` -- ADR record
  * `/docs/NETWORK_TLS.md` -- TLS posture detail
  * `/docs/development/RUNBOOK-TLS-PROBE.md` -- TLS handshake diagnosis
