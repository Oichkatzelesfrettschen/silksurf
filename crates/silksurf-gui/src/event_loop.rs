//! Synchronous XCB event pump that translates wire events into `Event`.
//!
//! EventLoop::run blocks on wait_for_event, translates XCB events into the
//! crate's Event enum, and calls the user handler. The handler returns
//! ControlFlow to continue or stop. A ClientMessage carrying WM_DELETE_WINDOW
//! reports Event::Close and terminates the pump.

use silksurf_core::SilkError;
use xcb::Xid;
use xcb::x;

use crate::input::{ControlFlow, Event};
use crate::window::XcbWindow;

/// XCB event pump. The struct is stateless and leaves room for later
/// per-loop queues, redraw debouncing, and frame timestamps.
#[derive(Default, Debug)]
pub struct EventLoop {
    /// Reserved for future use. The unit field keeps the struct
    /// non-empty so that adding fields later is not a breaking
    /// change to direct construction sites.
    _reserved: (),
}

impl EventLoop {
    /// Construct a new event loop. Cheap; no syscalls.
    #[must_use]
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
            // wait_for_event blocks until an event arrives. A clean window
            // manager shutdown may close the connection; that reports a
            // connection error and exits the pump normally.
            let xcb_event = match window.connection().wait_for_event() {
                Ok(ev) => ev,
                Err(xcb::Error::Connection(xcb::ConnError::Connection)) => {
                    // The display server closed the connection.
                    return Ok(());
                }
                Err(err) => {
                    return Err(SilkError::Engine(format!("xcb wait_for_event: {err}")));
                }
            };

            // translate_event updates cached window size on Resize.
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
/// Returns `None` for events the app does not surface.
fn translate_event(
    event: &xcb::Event,
    window: &mut XcbWindow,
    wm_delete: x::Atom,
) -> Option<Event> {
    match event {
        xcb::Event::X(x::Event::Expose(expose)) => Some(Event::Expose {
            width: u32::from(expose.width()),
            height: u32::from(expose.height()),
        }),
        xcb::Event::X(x::Event::KeyPress(key)) => Some(Event::KeyPress {
            keysym: u32::from(key.detail()),
        }),
        xcb::Event::X(x::Event::KeyRelease(key)) => Some(Event::KeyRelease {
            keysym: u32::from(key.detail()),
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
            let new_w = u32::from(configure.width());
            let new_h = u32::from(configure.height());
            // Window managers send ConfigureNotify for moves and resizes.
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
            // WM protocols used here carry 32-bit ClientMessage data.
            _ => None,
        },
        // Unhandled XCB events do not enter the normalized input stream.
        _ => None,
    }
}
