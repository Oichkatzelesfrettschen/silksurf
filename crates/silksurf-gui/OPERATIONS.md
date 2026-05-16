# silksurf-gui OPERATIONS

## Runtime tunables

| Variable | Effect |
|---|---|
| `DISPLAY` | X11 display to connect to. Passed to libxcb `xcb_connect(NULL, ...)`. Must be set; libxcb has no fallback. |

No other environment variables are consumed. All configuration is via code.

## Platform requirements

- Linux + X11 only (ADR-010). libxcb must be installed.
- The `xcb` crate links dynamically against `libxcb.so.1`. On Arch Linux: `sudo pacman -S libxcb`.
- No Wayland, macOS, or Windows support; these are explicit future work behind the same `XcbWindow`/`EventLoop` abstraction shape.

## Key types

- `XcbWindow` -- owns the `xcb::Connection`, window ID, GC, and cached WM atoms. `new(title, width, height)` opens the display and creates the window. `present(&[u32])` ships the ARGB framebuffer to the X server using `PutImage` (ZPixmap format, stripped for BIG-REQUESTS compatibility). Dropping the struct closes the connection.
- `EventLoop` -- stateless pump. `run(&mut window, handler)` blocks on `wait_for_event`, translates wire events into `Event`, calls the handler, and returns on `ControlFlow::Exit` or `Event::Close`.
- `Event` -- normalized event enum: `Expose { width, height }`, `KeyPress { keysym }`, `KeyRelease { keysym }`, `MousePress { button, x, y }`, `MouseRelease { button, x, y }`, `MouseMove { x, y }`, `Resize { width, height }`, `Close`.
- `ControlFlow` -- `Continue` or `Exit`. Return `Exit` from the handler to stop the loop.

## Pixel format

`present()` expects `&[u32]` with at least `width * height` elements. Each u32 is ARGB: `0xAARRGGBB` (high byte = alpha). On little-endian hosts (x86_64, AArch64-LE) this matches the in-memory byte order X11 expects for `ZPixmap` without a byte-swap pass.

## PutImage strip logic

A single `PutImage` for 1280x720 is ~3.5 MB. libxcb negotiates `BIG-REQUESTS` transparently; the `present()` method reads `get_maximum_request_length()` and slices the frame into row-aligned strips if needed. Each strip is a complete set of rows. On modern X servers the limit is > 16 MB so stripping does not trigger in practice.

## Common failure modes

### `XcbWindow::new` returns `SilkError::Engine("no display: ...")`

Cause: `DISPLAY` is unset or the socket path does not exist (headless server, no X session).

Fix: set `DISPLAY=:0` (or run `Xvfb :99 &; export DISPLAY=:99` for a virtual display). The caller (`silksurf-app --window`) prints a clean error and exits with code 1 rather than panicking.

### Window appears blank after `present()`

Cause: `present()` returns silently if `pixels.len() < width * height` (resize race). If the window is the correct size but still blank, the X server may not have received the flush.

Fix: verify the pixel buffer is exactly `width * height` u32s. The `present()` method calls `connection.flush()` at the end of every strip; errors from flush are silently discarded -- check for `SilkError` from `XcbWindow::new` or `EventLoop::run` instead.

### Handler not called for Expose events

Cause: the XCB event mask requests `EXPOSURE` events. The WM sends an Expose after mapping the window. If the event loop is not running yet (handler called before `run`), the event is queued and will be delivered on the first `wait_for_event` call.

Fix: call `event_loop.run(window, handler)` immediately after `window.present()`. The first Expose is a signal to repaint; the handler should call `window.present()` again to refresh after any WM unmap/remap cycle.

### WM close button terminates the process unexpectedly

Cause: `WM_DELETE_WINDOW` registration requires two `InternAtom` round-trips in `new()`. If these fail, the WM closes the window by tearing down the connection, which `wait_for_event` surfaces as `ConnError::Connection` -- treated as a graceful exit. Check stderr for `xcb intern WM_PROTOCOLS` or `xcb intern WM_DELETE_WINDOW` errors from `new()`.

## DoS bounds

`XcbWindow` makes no heap allocations proportional to event frequency. The pixel buffer is supplied by the caller; `present()` does not copy it (reinterprets as `&[u8]` via `slice::from_raw_parts` for the wire call). Event processing overhead is O(1) per event.
