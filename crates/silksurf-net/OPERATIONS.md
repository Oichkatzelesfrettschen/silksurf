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

## Resource bounds (TBD)

The current client has no max-body-size cap, no per-request timeout,
no max-connections cap. These are tracked in SNAZZY-WAFFLE roadmap
P8.S8 (DoS bounds). Until they land, do not point silksurf-app at
adversarial hosts.

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
