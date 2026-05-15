# silksurf-net Operations

## Cache directory

`silksurf-net::ResponseCache::with_disk(dir)` writes JSON-serialized
response entries to `dir`. The default directory used by
`silksurf-engine::SpeculativeRenderer` is:

  1. `$XDG_CACHE_HOME/silksurf/http` if `XDG_CACHE_HOME` is set
  2. `~/.cache/silksurf/http` otherwise

Filename: `<FxHash(url)>.json` (16-char hex). No URL bytes in the
filename, so path-traversal is structurally impossible.

To clear the cache:

```sh
rm -rf "${XDG_CACHE_HOME:-$HOME/.cache}/silksurf/http"
```

To inspect a cached entry:

```sh
ls -la "${XDG_CACHE_HOME:-$HOME/.cache}/silksurf/http/"
jq . "${XDG_CACHE_HOME:-$HOME/.cache}/silksurf/http/<hash>.json"
```

The on-disk schema (`CachedResponseDisk`) is `serde_json`-stable.
Records have `url`, `status`, `headers`, `body` (base64 if non-UTF-8),
`etag`, `last_modified`, `cached_at` (RFC 3339).

## Resource bounds (P8.S8)

| Constant                   | Default      | Enforcement site                                  | Failure mode                  |
|----------------------------|--------------|----------------------------------------------------|-------------------------------|
| `MAX_RESPONSE_BODY_BYTES`  | `16 MiB`     | `read_response()` accumulator check                | Returns `NetError`            |
| (alias) handshake timeout  | `30 s`       | `BasicClient::fetch` TCP `set_read_timeout` call    | Returns `NetError` on stall   |

The handshake timeout is sourced from
`silksurf_tls::MAX_TLS_HANDSHAKE_SECS` so the value lives in one place.
The body cap is checked after each `read()` chunk; oversized responses
are dropped, the connection closed, and the caller sees a recoverable
`NetError`.

Per-request total-deadline and max-connections caps remain on the
roadmap (still tracked under P8.S8). Until they land, do not point
silksurf-app at adversarial hosts.

## Tokio runtime

silksurf-net uses tokio with the minimal feature set: `rt`, `io-util`,
`net`, `time`. No multi-threaded runtime, no signal handling, no
process supervision. The synchronous `BasicClient::fetch` blocks the
calling thread on a per-call mini-runtime; long-running embedders
should keep tokio runtime ownership in their own code instead.

## Logging

Currently silent. The observability work (P8.S6) will add `tracing`
spans around fetch, cache lookup, and revalidation; until then, debug
via the `tls-probe` binary and direct `cargo run` traces.

## Cache eviction

Currently never evicted. The cache grows monotonically until manually
cleared. A SIZE-bounded LRU is a future option once usage patterns
solidify.
