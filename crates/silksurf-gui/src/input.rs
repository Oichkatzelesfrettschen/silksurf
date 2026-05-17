//! Normalized window input/output events and event-loop control.
//!
//! WHY: The XCB event types are wire-format wrappers (`*mut xcb_generic_event_t`
//! under the hood) that are awkward to pass around and impossible to construct
//! in tests without a live X connection. We project them into plain `Copy`
//! structs so the rest of the engine -- and the unit tests -- can manipulate
//! events without touching libxcb at all.
//!
//! WHAT: `Event` covers the seven event kinds the P6 slice cares about
//! (Expose / `KeyPress` / `KeyRelease` / `MouseMove` / `MousePress` / `MouseRelease` /
//! Resize / Close). `ControlFlow` is the handler return value that tells the
//! pump whether to keep running or exit.
//!
//! HOW: Construct an `Event` directly in tests; the `EventLoop` translates
//! XCB events into `Event` values before invoking the handler.

/// Normalized window input/output event surfaced to the application.
///
/// Field semantics mirror the XCB wire format: positions are i16 (X11
/// coordinates), sizes are u32 (after widening from the wire's u16 to
/// match the rest of the engine which uses u32 for pixel extents),
/// keysyms and buttons are kept as raw u32/u8 for now -- the keymap
/// translation layer is a P6.S4 follow-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// Window contents have been exposed and need redraw.
    /// width/height are the exposed-region dimensions.
    Expose { width: u32, height: u32 },
    /// A key was pressed. `keysym` is the raw X11 keycode for now;
    /// the keysym translation is queued for P6.S4 (xkbcommon).
    KeyPress { keysym: u32 },
    /// A key was released. Same `keysym` semantics as `KeyPress`.
    KeyRelease { keysym: u32 },
    /// Pointer moved to (x, y) in window-local coordinates.
    MouseMove { x: i16, y: i16 },
    /// Pointer button pressed. `button` matches X11 button numbers
    /// (1=left, 2=middle, 3=right, 4/5=scroll up/down).
    MousePress { button: u8, x: i16, y: i16 },
    /// Pointer button released. Same `button` semantics as `MousePress`.
    MouseRelease { button: u8, x: i16, y: i16 },
    /// Window has been resized by the user or window manager.
    Resize { width: u32, height: u32 },
    /// The window manager asked us to close (`WM_DELETE_WINDOW`).
    Close,
}

/// Tells the event loop whether to keep pumping or exit.
///
/// Modeled after the equivalent enum in winit so that future
/// callers familiar with that ecosystem feel at home; the
/// semantics are deliberately minimal here -- we only need
/// "keep going" and "stop".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlFlow {
    /// Continue pumping events.
    Continue,
    /// Drain pending events and return from `EventLoop::run`.
    Exit,
}
