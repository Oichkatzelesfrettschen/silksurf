# silksurf-net

HTTP/1.1 + HTTP/2 client, WebSocket probe transport, content-encoding
decode, persistent on-disk response cache, and conditional GET
(ETag / Last-Modified) revalidation. Built on `tokio` + `rustls` + `h2` +
`tokio-tungstenite` + `hickory-resolver`.

## Public API

  * `BasicClient` -- synchronous-style client (`fetch(&request) ->
    Result<Response, NetError>`).
  * `H2Client` -- HTTP/2 multiplexed client (`tokio_rustls` + `h2`).
  * `websocket_text_roundtrip` -- blocking WebSocket text exchange backed
    by a current-thread Tokio runtime.
  * `HttpRequest`, `HttpResponse`, `HttpMethod`, `HttpHeaders`.
  * `ResponseCache`, `CachedResponse`, `CachedResponseDisk` --
    in-memory cache + on-disk JSON persistence at
    `$XDG_CACHE_HOME/silksurf/http`. See operations notes in
    `OPERATIONS.md`.
  * `FetchOrigin` -- enum (`Network`, `Cache`, `RevalidatedCache`)
    surfaced through speculative rendering.
  * `NetError` -- crate-local error; `From<NetError> for
    silksurf_core::SilkError` at the bottom of `lib.rs`.
  * `content-encoding` -- default feature that advertises and decodes
    `br`, `gzip`, and `deflate`. Disable default features for a smaller
    identity-only embedded build.

## Cache semantics

  * Filename = `FxHash(url)` hex; no path traversal possible.
  * `put()` writes-through to disk best-effort (silent on I/O failure;
    in-memory entry still recorded).
  * `with_disk(dir)` loads all `*.json` on construction.
  * Second-run cache hit: ~9 us vs ~327 ms cold network fetch (chatgpt.com).

## Status

Functional for HTTP/1.1, HTTP/2, WebSocket text roundtrip, Brotli/gzip/deflate
response decode, persistent cache, conditional GET, and response-size bounds.
The Boa host exposes the text roundtrip through a minimal browser
`WebSocket` object. HTTP/3, persistent async browser sockets, OCSP stapling,
HSTS enforcement, and CORS are tracked in roadmap.

## See Also

  * `OPERATIONS.md` for env vars and cache-directory layout
  * `docs/NETWORK_TLS.md` for TLS posture
  * `docs/development/RUNBOOK-TLS-PROBE.md` for handshake debugging
  * `docs/design/THREAT-MODEL.md` Subsystem 1 + 7
