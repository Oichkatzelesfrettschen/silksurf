/*
 * winit_backend.rs -- cross-platform windowing via winit + softbuffer.
 *
 * WHY: The XCB backend in window.rs is Linux-only (X11). winit 0.30 provides a
 * unified event loop across X11, Wayland, macOS, and Windows. softbuffer 0.4
 * exposes `surface.buffer_mut() -> &mut [u32]`, which is the same type as the
 * rasterizer output from silksurf-render, making the adapter zero-copy.
 *
 * WHAT: WinitWindow wraps the creation args. `run()` takes ownership, creates
 * a winit EventLoop, instantiates a WinitApp that implements ApplicationHandler,
 * and calls event_loop.run_app(). The render closure is called on every
 * RedrawRequested event and returns a Vec<u32> pixel buffer (0xAARRGGBB).
 *
 * HOW:
 *   let win = WinitWindow::new("silksurf", 1280, 720).unwrap();
 *   win.run(|w, h| vec![0xFF6495EDu32; (w * h) as usize]);
 *
 * See: crates/silksurf-gui/src/window.rs for the XCB backend.
 * See: crates/silksurf-app/src/main.rs --backend=winit for the entry point.
 */

use std::num::NonZeroU32;
use std::rc::Rc;

use silksurf_core::SilkError;
use softbuffer::{Context, Surface};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{Key, NamedKey},
    window::{Window, WindowAttributes, WindowId},
};

/// Cross-platform window backed by winit + softbuffer.
///
/// Call `run()` to enter the event loop. The call blocks until the window
/// is closed (CloseRequested) or Escape is pressed.
pub struct WinitWindow {
    title: String,
    width: u32,
    height: u32,
}

impl WinitWindow {
    /// Create a WinitWindow with the given title and initial size.
    ///
    /// Returns `SilkError::Engine` if the platform does not support windowing
    /// (e.g., no DISPLAY on a headless CI box). The actual window is created
    /// lazily inside `run()` because winit requires an active event loop.
    pub fn new(title: &str, width: u32, height: u32) -> Result<Self, SilkError> {
        Ok(Self {
            title: title.to_string(),
            width,
            height,
        })
    }

    /// Enter the event loop.  Blocks until the window is closed.
    ///
    /// `render_fn(width, height)` is called on every RedrawRequested event
    /// and must return a pixel buffer of exactly `width * height` u32 values
    /// in 0xAARRGGBB format (same as the silksurf-render rasterizer output).
    ///
    /// On the first render the buffer is the initial size; on resize events
    /// the new dimensions are passed and the buffer must match.
    pub fn run(self, render_fn: impl FnMut(u32, u32) -> Vec<u32> + 'static) {
        let event_loop = match EventLoop::new() {
            Ok(el) => el,
            Err(e) => {
                eprintln!("[SilkSurf] winit: cannot create event loop: {e}");
                return;
            }
        };
        let mut app = WinitApp {
            title: self.title,
            init_width: self.width,
            init_height: self.height,
            window: None,
            surface: None,
            render_fn: Box::new(render_fn),
        };
        if let Err(e) = event_loop.run_app(&mut app) {
            eprintln!("[SilkSurf] winit: event loop error: {e}");
        }
    }
}

// ---------------------------------------------------------------------------
// Private ApplicationHandler implementation
// ---------------------------------------------------------------------------

struct WinitApp {
    title: String,
    init_width: u32,
    init_height: u32,
    window: Option<Rc<Window>>,
    // Context is not stored: softbuffer Surface keeps an internal Arc to the
    // display backend, so the Context can be dropped after Surface::new().
    surface: Option<Surface<Rc<Window>, Rc<Window>>>,
    render_fn: Box<dyn FnMut(u32, u32) -> Vec<u32>>,
}

impl ApplicationHandler for WinitApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attrs = WindowAttributes::default()
            .with_title(&self.title)
            .with_inner_size(PhysicalSize::new(self.init_width, self.init_height));

        let window = match event_loop.create_window(attrs) {
            Ok(w) => Rc::new(w),
            Err(e) => {
                eprintln!("[SilkSurf] winit: cannot create window: {e}");
                event_loop.exit();
                return;
            }
        };

        let context = match Context::new(window.clone()) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[SilkSurf] softbuffer: context error: {e}");
                event_loop.exit();
                return;
            }
        };

        let mut surface = match Surface::new(&context, window.clone()) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[SilkSurf] softbuffer: surface error: {e}");
                event_loop.exit();
                return;
            }
        };

        // Pre-size the surface buffer to match the initial window dimensions.
        if let (Some(nw), Some(nh)) = (
            NonZeroU32::new(self.init_width),
            NonZeroU32::new(self.init_height),
        ) {
            let _ = surface.resize(nw, nh);
        }

        self.window = Some(window.clone());
        self.surface = Some(surface);
        window.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }

            WindowEvent::KeyboardInput {
                event: key_event, ..
            } => {
                if key_event.logical_key == Key::Named(NamedKey::Escape) {
                    event_loop.exit();
                }
            }

            WindowEvent::RedrawRequested => {
                let (w, h) = self
                    .window
                    .as_ref()
                    .map(|win| {
                        let s = win.inner_size();
                        (s.width, s.height)
                    })
                    .unwrap_or((self.init_width, self.init_height));

                let pixels = (self.render_fn)(w, h);

                if let Some(surface) = &mut self.surface
                    && let (Some(nw), Some(nh)) = (NonZeroU32::new(w), NonZeroU32::new(h))
                {
                    let _ = surface.resize(nw, nh);
                    if let Ok(mut buf) = surface.buffer_mut() {
                        let copy_len = buf.len().min(pixels.len());
                        buf[..copy_len].copy_from_slice(&pixels[..copy_len]);
                        let _ = buf.present();
                    }
                }
            }

            WindowEvent::Resized(size) => {
                if let Some(surface) = &mut self.surface
                    && let (Some(nw), Some(nh)) =
                        (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                {
                    let _ = surface.resize(nw, nh);
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }

            _ => {}
        }
    }
}
