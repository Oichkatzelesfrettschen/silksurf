# Networking & TLS

This document describes the cleanroom networking layer and TLS configuration.

## Crates
- `crates/silksurf-net`: HTTP request/response types and a `NetClient` trait.
- `crates/silksurf-tls`: TLS provider abstraction and `rustls` adapter.

## Current Behavior
- `BasicClient` uses `RustlsProvider` by default.
- `TlsConfig` currently initializes an empty root store; certificate loading
  is not implemented yet.
- `NetClient::fetch` returns a not-implemented error placeholder.

## Configuration Plan
- Load OS root certificates into `TlsConfig`.
- Allow per-request TLS overrides (SNI, ALPN, timeouts).
- Add simple in-memory response cache for static assets.

## Engine Integration
- `silksurf-engine` will depend on `NetClient` to fetch HTML/CSS/JS.
- Network responses will feed the parser pipeline and JS task queue.
