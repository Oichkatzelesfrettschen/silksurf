# silksurf-app

`silksurf-app` is the integrated user-facing SilkSurf browser application. It
owns browser chrome, navigation, resource assembly, JavaScript/runtime pumping,
input routing, retained page state, and native presentation.

## Runtime modes

### Windowed browser (default)

```sh
cargo run -p silksurf-app -- https://example.com
```

The default path presents the winit browser shell, loads the URL, builds a live
DOM, executes supported scripts through `silksurf_js::SilkContext`, and lays out
and paints the page. Native input and host callbacks drive incremental repaint.
Wayland and X11 are supported through the winit backend; presenter selection is
handled by `silksurf-gui`.

### Headless static render

```sh
cargo run -p silksurf-app -- --headless https://example.com
```

This runs a one-shot fetch/parse/script/layout/raster pipeline and exits.

### Window startup and module limits

`--window` selects the default winit browser path. The browser presents its
address bar and navigation controls before fetching the initial document.
Fetch failure leaves a diagnostic with address entry and retry available.
Escape stops navigation.

Module execution uses compact defaults of four document roots, 512 KiB of
combined inline and external source, and 64 fetched module URLs. Set
`SILKSURF_MAX_MODULE_ROOTS`, `SILKSURF_MAX_MODULE_BYTES`, and
`SILKSURF_MAX_MODULE_URLS` to positive integers to select a larger allowance.
For example, `SILKSURF_MAX_MODULE_BYTES=8388608` admits 8 MiB of module source.
Invalid values fail startup. Source-byte limits bound admitted source; Boa
objects, decoded responses, DOM nodes, and rendered surfaces consume additional
memory. Peak RSS requires a separate measurement.

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
