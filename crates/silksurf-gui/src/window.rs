//! XCB-backed window: connection, drawable, and pixel-buffer presentation.
//!
//! The render pipeline produces a row-major ARGB pixel buffer. XcbWindow owns
//! the X11 connection, drawable, graphics context, close-protocol atoms, and
//! logical size. present() ships the u32 ARGB buffer through PutImage.

use silksurf_core::SilkError;
use xcb::Xid;
use xcb::x;

/// An X11 window that can present a CPU-rendered ARGB framebuffer.
///
/// One instance owns one `xcb::Connection`. The current API does not share a
/// connection across multiple windows.
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
    /// Cached `WM_PROTOCOLS` atom. This property carries `WM_DELETE_WINDOW`.
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
        // Connect to the default display and preserve "no display" as the
        // diagnostic surface for the app entry point.
        let (connection, screen_num) = xcb::Connection::connect(None)
            .map_err(|err| SilkError::Engine(format!("no display: {err}")))?;

        // Select the requested screen from the server setup.
        let setup = connection.get_setup();
        let screen = setup
            .roots()
            .nth(screen_num as usize)
            .ok_or_else(|| SilkError::Engine("xcb setup: no matching screen".to_string()))?;
        let root = screen.root();
        let root_visual = screen.root_visual();
        let depth = screen.root_depth();
        let white_pixel = screen.white_pixel();

        // Allocate XIDs for the window and graphics context.
        let window_id: x::Window = connection.generate_id();
        let gc: x::Gcontext = connection.generate_id();

        // X11 window dimensions travel as u16 values on the wire.
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
                // EXPOSURE reports redraw requests.
                // KEY_PRESS and KEY_RELEASE report keyboard input.
                // BUTTON_PRESS and BUTTON_RELEASE report mouse buttons.
                // POINTER_MOTION reports pointer movement.
                // STRUCTURE_NOTIFY reports resize and configure events.
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

        // Build a default GC bound to the new window. PutImage carries its
        // own pixels, and GraphicsExposures stays disabled after blits.
        connection.send_request(&x::CreateGc {
            cid: gc,
            drawable: x::Drawable::Window(window_id),
            value_list: &[x::Gc::GraphicsExposures(false)],
        });

        // WM_NAME supplies the title bar label.
        connection.send_request(&x::ChangeProperty {
            mode: x::PropMode::Replace,
            window: window_id,
            property: x::ATOM_WM_NAME,
            r#type: x::ATOM_STRING,
            data: title.as_bytes(),
        });

        // WM_PROTOCOLS advertises WM_DELETE_WINDOW so the event loop receives
        // a close event instead of a torn-down connection.
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

        // Map the window and flush setup requests to the server.
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

    /// Cached `WM_PROTOCOLS` atom.
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
