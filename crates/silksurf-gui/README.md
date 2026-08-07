# silksurf-gui

Windowing, event loop, and platform integration. SilkSurf renders into a
CPU-side ARGB framebuffer; this crate opens a window, ships that framebuffer to
the display server, and pumps input events back into the app.

## Backends

Both backends are feature-gated, and the crate exposes neither by default.

  * `winit-backend` -- cross-platform event loop with softbuffer pixel
    presentation over X11 and Wayland, plus a direct Wayland SHM presenter.
    `crates/silksurf-app` selects this one with `default-features = false`, so
    it is what a normal build runs. winit owns the window, which keeps the KMS
    backend out of this runtime surface.
  * `xcb-backend` -- the legacy Linux/X11 event loop with PutImage
    presentation, retained behind `silksurf-app`'s `xcb-backend` feature.
    ADR-010 recorded the original XCB-only, Linux-first decision; the winit
    path superseded it as the default.

## API

  * `input` -- normalized keyboard, mouse, and pointer event types, compiled
    into every configuration so backend tests run without a display.
  * `window::XcbWindow` and `event_loop::EventLoop` -- the XCB connection,
    drawable, and synchronous `wait_for_event` pump. Every XCB call lives in
    these two modules.
  * `winit_backend::WinitWindow` and its presenter types -- damage rects,
    retained buffer tags, presented-frame accounting, and `WinitWakeHandle`,
    which lets host or navigation work wake an event loop parked in
    `ControlFlow::Wait`.

## Testing

`tests/gui_basics.rs` covers the display-free surface. A test that needs a real
X server sits behind a `test-display` Cargo feature on the consuming crate,
which no in-tree crate sets.

## See Also

  * `docs/XCB_GUIDE.md` for XCB conventions
  * `docs/design/ARCHITECTURE-DECISIONS.md` AD-010 for the backend decision
