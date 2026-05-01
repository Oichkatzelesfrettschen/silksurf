# silksurf-app

The user-facing CLI binary for silksurf. Wires the engine pipeline,
networking, TLS, JS runtime, and the (in-progress) GUI into one entry
point.

## Binaries

  * **`silksurf-app`** -- the engine entry point. Loads a URL,
    parses HTML/CSS, runs JS, builds the layout tree, rasterizes,
    and either dumps the framebuffer or (eventually, P6) opens an
    XCB window. Speculative rendering and persistent on-disk cache
    are wired in; second-run cache hit is ~9 us.
  * **`tls-probe`** -- 982-line TLS handshake diagnostic with DANE
    TLSA probe, X.509 chain display, and explicit RCA for the four
    canonical UnknownIssuer failure classes. See
    `docs/development/RUNBOOK-TLS-PROBE.md`.

## Flags

```sh
silksurf-app <url>                       # render to framebuffer
silksurf-app --speculative <url>         # enable speculative pre-render
silksurf-app --tls-ca-file <path> <url>  # supply extra CA certs at runtime
```

## Status

Headless render works end-to-end. GUI mode (window-open + paint) is
queued in roadmap P6 (`crates/silksurf-gui` is currently a one-line stub
backed by ADR-010 XCB-only Linux-first).

## See Also

  * `/docs/ARCHITECTURE.md` -- pipeline overview
  * `/docs/PERFORMANCE.md` -- measured 9.5 us steady-state path
  * `/docs/development/RUNBOOK-TLS-PROBE.md` -- TLS diagnosis
