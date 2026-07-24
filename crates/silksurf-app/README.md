# silksurf-app

`silksurf-app` is the integrated user-facing SilkSurf browser application. It
owns browser chrome, navigation, resource assembly, JavaScript/runtime pumping,
input routing, retained page state, and native presentation.

## Runtime modes

### Windowed browser (default)

```sh
cargo run -p silksurf-app -- https://example.com
```

The default path loads the URL, builds a live DOM, executes supported scripts
through `silksurf_js::SilkContext`, lays out and paints the page, opens a winit
window, and routes native input and host callbacks through incremental repaint.
Wayland and X11 are supported through the winit backend; presenter selection is
handled by `silksurf-gui`.

### Headless static render

```sh
cargo run -p silksurf-app -- --headless https://example.com
```

This runs a one-shot fetch/parse/script/layout/raster pipeline and exits.

### Legacy XCB probe

`--window` uses the optional `xcb-backend` feature and is retained as an
isolated legacy presenter probe. It is not the default browser path.

## Common flags

```text
--headless
--display-backend=auto|wayland|x11
--speculative / -s
--tls-ca-file <path>
--platform-verifier
--insecure / -k
```

Use `make gui-probe` and the focused `gui-probe-*` targets for scripted live
Wayland/X11 evidence.

## Current capabilities

The application integrates:

- asynchronous navigation workers and stop/reload/history controls,
- external CSS, scripts, modules within current caps, and decoded images,
- shared partitioned cookies between HTTP and `document.cookie`,
- Boa-backed page JavaScript and host callbacks,
- native pointer/keyboard dispatch into page event listeners,
- focused text controls,
- scrolling and retained viewport caches,
- same-box text damage and fused relayout/repaint,
- Wayland SHM and softbuffer presentation paths,
- optional accessibility snapshot generation.

## Current limitations

The app remains a single-view shell. `BrowserState` holds one page runtime,
history vector, focused input, and frame. The page DOM, JavaScript context,
layout state, display list, and raster scratch share the browser process; there
is no renderer crash/sandbox boundary yet.

Tabs, windows, profiles, downloads, permissions, file chooser, full
selection/clipboard/IME behavior, session restore, and compatibility-engine
backends belong to the browser functionalization program in
`docs/roadmaps/BROWSER-FUNCTIONALIZATION-ACTION-PLAN.md` and issue #50.

## Binaries

- `silksurf-app` -- integrated browser/headless renderer
- `tls-probe` -- TLS/DANE/X.509 diagnostic, behind `tls-probe`
- `h2-fetch-probe` -- HTTP/2 diagnostic client

## Related documents

- `docs/STATUS.md`
- `docs/ARCHITECTURE.md`
- `docs/JS_ENGINE.md`
- `docs/PERFORMANCE.md`
- `docs/development/RUNBOOK-TLS-PROBE.md`
