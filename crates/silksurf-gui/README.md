# silksurf-gui

Windowing, event loop, and platform integration. **Currently a stub**
(one doc-comment line in `lib.rs`); implementation queued in
SNAZZY-WAFFLE roadmap P6.

## Decision

  * **Backend = XCB only, Linux first** (see ADR-010). Wayland, macOS,
    Windows are explicit future work behind the same backend trait.

## Planned API

  * `Window` -- platform window handle.
  * `EventLoop` -- input + redraw event pump.
  * `Input` -- normalized keyboard / mouse / pointer events.

## Status

Empty. The implementation work in P6 will:

  1. Author `crates/silksurf-gui/src/{window,event_loop,input}.rs`.
  2. Wire `silksurf-app --window` to open an XCB window, attach the
     renderer's framebuffer via SHM pixmap, and pump the event loop.
  3. Add screenshot-diff smoke against fixture pages.

See `docs/XCB_GUIDE.md` for the existing XCB conventions.
