//! Synchronous XCB event pump that translates wire events into `Event`.
//!
//! WHY: We want to keep the event loop separate from the window so it can
//! be reused for multi-window setups in the future and so it can be
//! mocked/tested without touching libxcb. The loop is intentionally
//! single-threaded and synchronous -- the speculative networking layer
//! does its own threading, and the rasterizer is invoked from inside
//! the handler closure so the GPU/CPU back-pressure naturally throttles
//! event processing.
//!
//! WHAT: `EventLoop::run` blocks on `wait_for_event`, translates each
//! XCB event into our `Event` enum, and calls the user handler. The
//! handler returns `ControlFlow` to keep going or stop. A
//! `ClientMessage` whose payload matches the cached
//! `WM_DELETE_WINDOW` atom is reported as `Event::Close` and ends the
//! pump unconditionally regardless of what the handler returns -- the
//! rationale is that ignoring a WM close request is bad behaviour.
//!
//! HOW: Construct with `EventLoop::new()`, call `run(window, handler)`
//! once. The loop terminates on Close, on `ControlFlow::Exit` from the
//! handler, or on a connection error.

use silksurf_core::SilkError;
use xcb::Xid;
use xcb::x;

use crate::input::{ControlFlow, Event};
use crate::window::XcbWindow;

/// XCB event pump. Currently stateless -- the struct exists so that
/// future per-loop state (deferred work queues, redraw debouncing,
/// frame timestamps for vsync) can be added without breaking the API.
#[derive(Default, Debug)]
pub struct EventLoop {
    /// Reserved for future use. The unit field keeps the struct
    /// non-empty so that adding fields later is not a breaking
    /// change to direct construction sites.
    _reserved: (),
}

impl EventLoop {
    /// Construct a new event loop. Cheap; no syscalls.
    pub fn new() -> Self {
        Self { _reserved: () }
    }

    /// Pump events from `window` and dispatch them to `handler` until
    /// the handler returns `ControlFlow::Exit` or a Close event arrives.
    ///
    /// The handler may call `window.present(...)` from inside its body
    /// -- that is in fact the expected pattern for redraw on Expose.
    pub fn run<F>(&mut self, window: &mut XcbWindow, mut handler: F) -> Result<(), SilkError>
    where
        F: FnMut(Event) -> ControlFlow,
    {
        let wm_delete = window.wm_delete_window();
        loop {
            // wait_for_event blocks until an event arrives. On a clean
            // WM-shutdown (we did not register WM_DELETE_WINDOW, or the
            // connection was forcibly closed) this returns
            // Err(Error::Connection(ConnError::Connection)) -- treat
            // that as a graceful exit rather than a hard failure.
            let xcb_event = match window.connection().wait_for_event() {
                Ok(ev) => ev,
                Err(xcb::Error::Connection(xcb::ConnError::Connection)) => {
                    // Connection torn down by the server / WM. Exit cleanly.
                    return Ok(());
                }
                Err(err) => {
                    return Err(SilkError::Engine(format!("xcb wait_for_event: {err}")));
                }
            };

            // Translate. translate_event may need to update window
            // size on Resize, so it takes &mut window.
            let translated = translate_event(&xcb_event, window, wm_delete);

            if let Some(event) = translated {
                let was_close = matches!(event, Event::Close);
                let flow = handler(event);
                if was_close || flow == ControlFlow::Exit {
                    return Ok(());
                }
            }
        }
    }
}

/// Translate a single XCB event into our normalized `Event`.
///
/// Returns `None` for events we do not surface (KeymapNotify, MapNotify,
/// ReparentNotify, etc.) -- the loop just polls the next one.
fn translate_event(
    event: &xcb::Event,
    window: &mut XcbWindow,
    wm_delete: x::Atom,
) -> Option<Event> {
    match event {
        xcb::Event::X(x::Event::Expose(expose)) => Some(Event::Expose {
            width: expose.width() as u32,
            height: expose.height() as u32,
        }),
        xcb::Event::X(x::Event::KeyPress(key)) => Some(Event::KeyPress {
            keysym: key.detail() as u32,
        }),
        xcb::Event::X(x::Event::KeyRelease(key)) => Some(Event::KeyRelease {
            keysym: key.detail() as u32,
        }),
        xcb::Event::X(x::Event::ButtonPress(button)) => Some(Event::MousePress {
            button: button.detail(),
            x: button.event_x(),
            y: button.event_y(),
        }),
        xcb::Event::X(x::Event::ButtonRelease(button)) => Some(Event::MouseRelease {
            button: button.detail(),
            x: button.event_x(),
            y: button.event_y(),
        }),
        xcb::Event::X(x::Event::MotionNotify(motion)) => Some(Event::MouseMove {
            x: motion.event_x(),
            y: motion.event_y(),
        }),
        xcb::Event::X(x::Event::ConfigureNotify(configure)) => {
            let new_w = configure.width() as u32;
            let new_h = configure.height() as u32;
            // Only report a Resize when the size actually changed -- WMs
            // send ConfigureNotify for moves too.
            if new_w != window.width() || new_h != window.height() {
                window.set_size(new_w, new_h);
                Some(Event::Resize {
                    width: new_w,
                    height: new_h,
                })
            } else {
                None
            }
        }
        xcb::Event::X(x::Event::ClientMessage(message)) => match message.data() {
            x::ClientMessageData::Data32([atom_id, ..]) => {
                if atom_id == wm_delete.resource_id() {
                    Some(Event::Close)
                } else {
                    None
                }
            }
            // 8- and 16-bit ClientMessage formats are not used by
            // any WM protocol we care about; ignore.
            _ => None,
        },
        // Ignore everything else -- KeymapNotify, MapNotify,
        // ReparentNotify, GraphicsExposure, NoExposure, ...
        _ => None,
    }
}
