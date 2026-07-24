# silksurf-net

`silksurf-net` owns SilkSurf's HTTP, cookie, response-cache, WebSocket, and
Server-Sent Events transport surfaces. It is built on Tokio, rustls, `h2`,
`tokio-tungstenite`, and Hickory DNS components, while retaining synchronous
facades where the current application integration expects them.

## Public API

- `BasicClient` -- synchronous-style HTTP request facade.
- `H2Client` -- HTTP/2 multiplexed client used by batch/prefetch paths.
- `WebSocketSession` -- persistent duplex WebSocket transport; a background
  current-thread Tokio runtime owns the socket while the caller polls lifecycle
  and inbound frames without blocking the JavaScript thread.
- `websocket_text_roundtrip` -- one-shot diagnostic/test transport.
- `SseSubscription` and `SseParser` -- incremental Server-Sent Events transport
  and WHATWG field parser.
- `HttpRequest`, `HttpResponse`, `HttpMethod`, and `HttpHeaders`.
- `PartitionedCookieStore` and request cookie context.
- `ResponseCache`, `CachedResponse`, and `CachedResponseDisk` -- memory and
  best-effort JSON disk persistence under `$XDG_CACHE_HOME/silksurf/http`.
- `NetError` -- crate-local error converted to `silksurf_core::SilkError` at
  workspace boundaries.

The default `content-encoding` feature advertises and decodes Brotli, gzip, and
deflate. Disable default features for a smaller identity-only embedded build.

## Current application integration

- document and subresource fetches use the blocking `BasicClient` facade from
  navigation/resource workers rather than the GUI thread,
- same-host batch resource paths can use `H2Client`,
- `fetch()` completions cross the SilkSurf JavaScript host queue,
- persistent WebSocket open/message/close/error events and EventSource events
  drain through the host callback path,
- the HTTP cookie jar is shared with the `document.cookie` bridge and partitioned
  by top-level/resource site,
- conditional GET and persistent response caching support cache-first reloads.

## Cache semantics

- cache filenames are hexadecimal URL hashes and cannot contain path separators,
- `put()` records the in-memory entry even when best-effort disk persistence
  fails,
- `with_disk(dir)` loads persisted JSON entries,
- retained measurements report a second-run cache hit around 9 us versus a
  hundreds-of-milliseconds cold fetch on the cited host; this is a historical
  measurement, not a network guarantee.

## Known limitations

- JavaScript `fetch` does not yet use HTTP/2 on the ordinary single-request path,
- socket-level streaming response bodies are incomplete; current fetch stream
  delivery can slice a completed buffered body,
- abort after a worker request has started does not fully cancel mid-flight I/O,
- XHR migration to the asynchronous completion queue is incomplete,
- EventSource HTTPS integration and reconnect policy are incomplete,
- WebSocket/EventSource non-DOM EventTarget parity is incomplete,
- CORS, CSP/SRI integration, HSTS enforcement, OCSP policy, and HTTP/3 remain
  open,
- redirect-hop SameSite reclassification and public-suffix cookie-domain
  rejection remain open.

Persistent WebSocket transport and an SSE subscription/parser exist today; they
must not be described as absent. Their remaining browser-semantics and security
work is tracked in `docs/roadmaps/SPA-CAPABILITY-ROADMAP.md`, `docs/STATUS.md`,
and the browser functionalization action plan.

## Related documents

- `OPERATIONS.md`
- `docs/NETWORK_TLS.md`
- `docs/development/RUNBOOK-TLS-PROBE.md`
- `docs/design/THREAT-MODEL.md`
- `docs/roadmaps/BROWSER-FUNCTIONALIZATION-ACTION-PLAN.md`
