//! XCB-backed window: connection, drawable, and pixel-buffer presentation.
//!
//! WHY: The render pipeline produces an ARGB pixel buffer (one u32 per pixel,
//! row-major). To make those pixels visible we need a real X11 drawable. We
//! deliberately do not use SHM (`XShm`) yet -- the raw `PutImage` path is
//! simpler, has no shared-memory teardown semantics to get wrong, and is
//! fast enough for the P6 slice (1280x720 @ ~3.5MB per frame is a single
//! request thanks to BIG-REQUESTS, which xcb negotiates by default).
//!
//! WHAT: `XcbWindow` owns the `Connection`, the screen index, the X window
//! ID, a graphics context for `PutImage`, the cached `WM_DELETE_WINDOW`
//! atom, and the current logical size. The `present()` method ships a u32
//! ARGB buffer to the server; `new()` opens the display, creates the
//! window, sets `WM_NAME`, and registers `WM_DELETE_WINDOW` so the WM close
//! button funnels through our event loop instead of slamming the
//! connection shut.
//!
//! HOW: Drop the `XcbWindow` to release everything -- libxcb closes the
//! connection in its own `Drop` impl on the inner `Connection`, which
//! also takes the GC and window IDs with it server-side.

use silksurf_core::SilkError;
use xcb::Xid;
use xcb::x;

/// An X11 window that can present a CPU-rendered ARGB framebuffer.
///
/// One instance owns one `xcb::Connection`. We do not currently support
/// sharing a connection between multiple windows -- that pattern is
/// useful for tabbed/multi-window browsers and is a P6.S4 follow-up.
pub struct XcbWindow {
    /// Live X server connection. Dropping this closes the socket.
    connection: xcb::Connection,
    /// Default screen index for this connection.
    screen_num: i32,
    /// Server-side window ID.
    window_id: x::Window,
    /// Graphics context bound to `window_id`. Required by `PutImage` even
    /// though we do not configure foreground/background -- the GC carries
    /// the function (`GXcopy` by default), the plane mask, and the clip
    /// region (none, by default).
    gc: x::Gcontext,
    /// Cached `WM_DELETE_WINDOW` atom. The event loop compares incoming
    /// `ClientMessage` payloads against this atom to detect the close
    /// button being clicked in the window manager titlebar.
    wm_delete_window: x::Atom,
    /// Cached `WM_PROTOCOLS` atom -- the property we wrote
    /// `WM_DELETE_WINDOW` into. Kept for symmetry / future use.
    wm_protocols: x::Atom,
    /// Logical width in pixels. Updated on Resize.
    width: u32,
    /// Logical height in pixels. Updated on Resize.
    height: u32,
    /// Color depth (`root_depth` from the screen). Needed by `PutImage`
    /// every call, so we cache it once at construction.
    depth: u8,
}

impl XcbWindow {
    /// Open the default display, create a 32-bpp window, register the
    /// close-button protocol, and map it.
    ///
    /// On systems without `DISPLAY` set (or where libxcb cannot connect),
    /// returns `SilkError::Engine("no display: ...")` so the caller can
    /// fall back to a headless mode rather than panicking.
    pub fn new(title: &str, width: u32, height: u32) -> Result<Self, SilkError> {
        // 1) Open the connection. This is the only failure mode that
        //    distinguishes "no display" from "everything else", so map
        //    it explicitly so the app entry point can detect it.
        let (connection, screen_num) = xcb::Connection::connect(None)
            .map_err(|err| SilkError::Engine(format!("no display: {err}")))?;

        // 2) Pull the requested screen out of the setup. nth() returns
        //    None if the server reported zero screens (impossible in
        //    practice but we still surface it as an Engine error rather
        //    than panicking).
        let setup = connection.get_setup();
        let screen = setup
            .roots()
            .nth(screen_num as usize)
            .ok_or_else(|| SilkError::Engine("xcb setup: no matching screen".to_string()))?;
        let root = screen.root();
        let root_visual = screen.root_visual();
        let depth = screen.root_depth();
        let white_pixel = screen.white_pixel();

        // 3) Allocate XIDs for the window and the graphics context.
        let window_id: x::Window = connection.generate_id();
        let gc: x::Gcontext = connection.generate_id();

        // Width/height fit into u16 on the X11 wire. We clamp at u16::MAX
        // (65535) which is well above any realistic screen size; this
        // never triggers in practice but keeps the cast lossless.
        let wire_w: u16 = width.min(u32::from(u16::MAX)) as u16;
        let wire_h: u16 = height.min(u32::from(u16::MAX)) as u16;

        connection.send_request(&x::CreateWindow {
            depth: x::COPY_FROM_PARENT as u8,
            wid: window_id,
            parent: root,
            x: 0,
            y: 0,
            width: wire_w,
            height: wire_h,
            border_width: 0,
            class: x::WindowClass::InputOutput,
            visual: root_visual,
            value_list: &[
                x::Cw::BackPixel(white_pixel),
                // EXPOSURE              -- redraw events
                // KEY_PRESS/RELEASE     -- keyboard
                // BUTTON_PRESS/RELEASE  -- mouse buttons
                // POINTER_MOTION        -- pointer movement
                // STRUCTURE_NOTIFY      -- resize / configure events
                x::Cw::EventMask(
                    x::EventMask::EXPOSURE
                        | x::EventMask::KEY_PRESS
                        | x::EventMask::KEY_RELEASE
                        | x::EventMask::BUTTON_PRESS
                        | x::EventMask::BUTTON_RELEASE
                        | x::EventMask::POINTER_MOTION
                        | x::EventMask::STRUCTURE_NOTIFY,
                ),
            ],
        });

        // 4) Build a default GC bound to the new window. We do not set
        //    foreground/background -- PutImage carries its own pixels --
        //    but we suppress GraphicsExposures so we do not get spurious
        //    NoExposure events back after every blit.
        connection.send_request(&x::CreateGc {
            cid: gc,
            drawable: x::Drawable::Window(window_id),
            value_list: &[x::Gc::GraphicsExposures(false)],
        });

        // 5) Set WM_NAME so the title bar shows our title rather than
        //    the WM's "Untitled" placeholder.
        connection.send_request(&x::ChangeProperty {
            mode: x::PropMode::Replace,
            window: window_id,
            property: x::ATOM_WM_NAME,
            r#type: x::ATOM_STRING,
            data: title.as_bytes(),
        });

        // 6) Look up WM_PROTOCOLS and WM_DELETE_WINDOW, then advertise
        //    that we handle WM_DELETE_WINDOW. Without this, the WM
        //    closes our window by terminating the connection, which
        //    libxcb surfaces as `ConnError::Connection` -- ugly.
        let cookie_protocols = connection.send_request(&x::InternAtom {
            only_if_exists: false,
            name: b"WM_PROTOCOLS",
        });
        let cookie_delete = connection.send_request(&x::InternAtom {
            only_if_exists: false,
            name: b"WM_DELETE_WINDOW",
        });
        let wm_protocols = connection
            .wait_for_reply(cookie_protocols)
            .map_err(|err| SilkError::Engine(format!("xcb intern WM_PROTOCOLS: {err}")))?
            .atom();
        let wm_delete_window = connection
            .wait_for_reply(cookie_delete)
            .map_err(|err| SilkError::Engine(format!("xcb intern WM_DELETE_WINDOW: {err}")))?
            .atom();

        connection.send_request(&x::ChangeProperty {
            mode: x::PropMode::Replace,
            window: window_id,
            property: wm_protocols,
            r#type: x::ATOM_ATOM,
            data: &[wm_delete_window],
        });

        // 7) Map (show) the window and flush. Without flush, the server
        //    sees nothing until the first event-loop blocking call.
        connection.send_request(&x::MapWindow { window: window_id });
        connection
            .flush()
            .map_err(|err| SilkError::Engine(format!("xcb flush (window setup): {err}")))?;

        Ok(Self {
            connection,
            screen_num,
            window_id,
            gc,
            wm_delete_window,
            wm_protocols,
            width,
            height,
            depth,
        })
    }

    /// X11 window ID. Useful for callers that want to make their own
    /// requests against the window (custom properties, sub-windows,
    /// etc.) without going through this wrapper.
    #[must_use] 
    pub fn window_id(&self) -> x::Window {
        self.window_id
    }

    /// Default screen index for this connection.
    #[must_use] 
    pub fn screen_num(&self) -> i32 {
        self.screen_num
    }

    /// Cached `WM_DELETE_WINDOW` atom -- the event loop needs this
    /// to recognise close-button `ClientMessage` events.
    pub(crate) fn wm_delete_window(&self) -> x::Atom {
        self.wm_delete_window
    }

    /// Cached `WM_PROTOCOLS` atom. Currently unused outside `new()`
    /// but kept in the API surface so that future protocol additions
    /// (`NET_WM_PING`, `NET_WM_SYNC_REQUEST`, ...) do not need another
    /// `InternAtom` round-trip.
    #[must_use] 
    pub fn wm_protocols(&self) -> x::Atom {
        self.wm_protocols
    }

    /// Width in pixels. Tracks the most recent Resize event observed
    /// by the event loop -- not necessarily the server-side state if
    /// no events have been pumped yet.
    #[must_use] 
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels. Same caveat as `width()`.
    #[must_use] 
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Borrow the underlying connection for advanced use cases.
    #[must_use] 
    pub fn connection(&self) -> &xcb::Connection {
        &self.connection
    }

    /// Update the cached size. Called by the event loop on Resize so
    /// that subsequent `present()` calls submit the right wire dimensions.
    pub(crate) fn set_size(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
    }

    /// Present a CPU-rendered ARGB framebuffer.
    ///
    /// `pixels` must be at least `self.width * self.height` u32s long;
    /// each u32 is interpreted as 0xAARRGGBB which matches the
    /// `silksurf_render` framebuffer convention.
    ///
    /// On the X11 wire each pixel is sent little-endian: byte 0 = B,
    /// byte 1 = G, byte 2 = R, byte 3 = A. This matches the in-memory
    /// layout of `[u32]` on little-endian hosts; on big-endian hosts
    /// the bytes would need swapping. `SilkSurf` is `x86_64` / aarch64-LE
    /// only (both little-endian, see ADR-008), so we do not pay for
    /// a swap pass here.
    ///
    /// WHY no SHM yet: the unshared `PutImage` path goes through a
    /// single iovec write per call -- libxcb handles BIG-REQUESTS
    /// transparently up to a server-negotiated limit (~16 MiB on
    /// modern X servers, far above our 3.5 MiB worst case at 1280x720).
    /// SHM gives us roughly a 2-3x speedup but adds a multi-step
    /// teardown contract (segment attach/detach, `ShmCompletion` event)
    /// that we do not want to land at the same time as the very
    /// first window-mode integration.
    pub fn present(&self, pixels: &[u32]) {
        let pixel_count = self.width as usize * self.height as usize;
        if pixels.len() < pixel_count {
            // We deliberately do NOT panic here: callers may legitimately
            // pass an undersized buffer during a transient resize race
            // (server has resized the window, app has not re-rendered yet).
            // Drop the frame; the next Expose will retry.
            return;
        }

        // Reinterpret &[u32] as &[u8] without copying.  Each u32 occupies
        // 4 bytes; total length is pixel_count * 4.
        // SAFETY: pixels has at least pixel_count u32s (checked above);
        // u32 has alignment 4 which is >= alignment 1 required by u8;
        // we only borrow for the duration of the slice and the resulting
        // slice is read-only -- no aliasing concerns.
        let byte_view: &[u8] =
            unsafe { std::slice::from_raw_parts(pixels.as_ptr().cast::<u8>(), pixel_count * 4) };

        // Maximum bytes per request (in 4-byte units * 4). Subtract a
        // generous header allowance (~64 bytes) so we never bump the
        // limit. On a server with BIG-REQUESTS this is several MiB; on
        // a server without, it is ~256 KiB minus the header. Either way
        // we slice into row-aligned strips so each strip is a complete
        // PutImage.
        let max_units = self.connection.get_maximum_request_length();
        // get_maximum_request_length is in 4-byte units; convert to bytes
        // and reserve 64 bytes for the request header itself.
        let max_bytes_per_request = (max_units as usize).saturating_mul(4).saturating_sub(64);

        let row_bytes = self.width as usize * 4;
        if row_bytes == 0 {
            return;
        }
        // Rows per strip: at least 1 (so we make progress even if a single
        // row exceeds the limit, in which case the server will reject and
        // we will see ClosedReqLenExceed -- this is a 16K-pixel-wide
        // window which is not a configuration we support).
        let rows_per_strip = (max_bytes_per_request / row_bytes).max(1);

        let mut row_offset: usize = 0;
        let total_rows = self.height as usize;
        while row_offset < total_rows {
            let rows_this_strip = rows_per_strip.min(total_rows - row_offset);
            let strip_start_byte = row_offset * row_bytes;
            let strip_end_byte = strip_start_byte + rows_this_strip * row_bytes;
            let strip = &byte_view[strip_start_byte..strip_end_byte];

            self.connection.send_request(&x::PutImage {
                format: x::ImageFormat::ZPixmap,
                drawable: x::Drawable::Window(self.window_id),
                gc: self.gc,
                width: self.width.min(u32::from(u16::MAX)) as u16,
                height: rows_this_strip.min(u16::MAX as usize) as u16,
                dst_x: 0,
                dst_y: row_offset.min(i16::MAX as usize) as i16,
                left_pad: 0,
                depth: self.depth,
                data: strip,
            });

            row_offset += rows_this_strip;
        }

        // Flush so the pixels actually reach the server before the
        // next event-loop blocking call.  Ignore errors: the next
        // event poll will surface the same condition more clearly.
        let _ = self.connection.flush();
    }
}

impl std::fmt::Debug for XcbWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("XcbWindow")
            .field("screen_num", &self.screen_num)
            .field("window_id", &self.window_id.resource_id())
            .field("width", &self.width)
            .field("height", &self.height)
            .field("depth", &self.depth)
            .finish()
    }
}
