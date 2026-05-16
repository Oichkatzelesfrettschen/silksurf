# silksurf-app OPERATIONS

## Runtime tunables

| Variable | Effect |
|---|---|
| `RUST_LOG` | Log filter for tracing subscriber. Default: `silksurf=info`. Example: `RUST_LOG=silksurf=debug`. Writes to stderr. |
| `DISPLAY` | X11 display target for `--window` mode. Must be set when using XCB window output. |

## CLI flags

| Flag | Effect |
|---|---|
| `<URL>` | URL to fetch and render. Defaults to `https://example.com`. |
| `--insecure` / `-k` | Disable TLS certificate verification. Prints a warning to stderr. |
| `--tls-ca-file <path>` | Append a PEM CA bundle to the default Mozilla trust store. Accepts both space form and `=` form. |
| `--platform-verifier` | Use the system TLS verifier rather than rustls. Requires the `platform-verifier` feature. |
| `--speculative` / `-s` | After a cache hit, spawn a background revalidation GET while the render pipeline runs. |
| `--window` | Open an XCB window and present the rendered frame. Exits cleanly on window close or Escape (keysym 0x09). |

## Pipeline order

1. Fetch URL (ResponseCache hit = 0ms; fresh = live HTTPS with cache write).
2. Parse HTML into DOM.
3. Extract inline CSS from `<style>` tags; fetch `<link rel="stylesheet">` URLs in parallel (HTTP/2 multiplexing).
4. Parse or cache-hit stylesheet (`StylesheetCache`: full parse on first hit, `intern_rules` on repeat).
5. Create JS VM and install DOM bridge.
6. Execute inline `<script>` tags (skips bundles > 256 KB).
7. Run one microtask/timer tick.
8. Fused style+layout+paint: single BFS pass over the post-JS DOM.
9. Tile-parallel rasterization via Rayon into a reusable `Vec<u8>`.
10. (If `--speculative` and cache hit) join background revalidation; run incremental re-render on DOM diff.

## Common failure modes

### `--window`: cannot open display

Cause: `DISPLAY` not set, or the X server is not reachable.

Fix: ensure the X11 session is running and `DISPLAY` is exported. On a headless box, use `Xvfb :99 &; export DISPLAY=:99`.

### TLS: UnknownIssuer

Cause: server certificate signed by a CA not in the Mozilla root bundle.

Fix: pass `--tls-ca-file /path/to/ca.pem` for a private CA, or `--platform-verifier` to delegate to the OS trust store. See `docs/development/RUNBOOK-TLS-PROBE.md`.

### Script execution skipped ("bundle too large")

Cause: inline `<script>` body exceeds 256 KB -- the app treats this as a bundled JS file and skips it to avoid hanging on large React or webpack output.

Fix: external scripts loaded via `<script src="...">` are always skipped (not fetched in this build). Inline init scripts under 256 KB execute normally.

### CSS parse returns empty stylesheet

Cause: `get_or_parse_stylesheet` returns `None` when the interner fails to intern the rules into the new DOM's interner. The fallback constructs an empty `Stylesheet`.

Fix: ensure `dom.materialize_resolve_table()` is called after the parse phase (done automatically by `silksurf-html::into_dom`). Check `RUST_LOG=silksurf=debug` output for any `Io` or `Css` error preceding the warning.

### Fused pipeline: zero display items

Cause: viewport `Rect` has `height = 0`, or the document root has `display: none`.

Fix: the hardcoded viewport is 1280x800; this failure indicates a DOM parse error that produced an empty document. Check the `HTML parse error` line in stderr.

## Binaries

| Binary | Purpose |
|---|---|
| `silksurf-app` | Main entry point: full fetch-parse-style-layout-render pipeline. |
| `tls-probe` | Standalone TLS diagnostic: connects to a host, reports certificate chain, handshake version, ALPN, and any errors. See `docs/development/RUNBOOK-TLS-PROBE.md`. |

## Memory model

The global allocator is `mimalloc` (thread-local free lists, page segregation). The primary purpose is reducing latency on CSS tokenizer allocations. No OOM hook is installed: mimalloc aborts the process on OOM in release builds.

## DoS bounds

| Input path | Bound |
|---|---|
| Inline script size | 256 KB per script; larger scripts are silently skipped |
| Response body | Bounded by `silksurf-net` fetch limit (see silksurf-net OPERATIONS.md) |
| CSS size | No explicit cap; bounded by available heap |
| DOM node count | Bounded by HTML parser `MAX_TOKENS_PER_FEED` in silksurf-html |
