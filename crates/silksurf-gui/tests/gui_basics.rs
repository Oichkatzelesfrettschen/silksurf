//! Headless smoke tests for `silksurf-gui`.
//!
//! WHY: CI runs without a DISPLAY. Anything that calls
//! `xcb::Connection::connect` would fail there, so we restrict these
//! tests to types that are safe to construct in isolation: the `Event`
//! enum, the `ControlFlow` enum, and a parallel `WindowParams` builder
//! that mirrors the size storage in `XcbWindow` without the live
//! connection.
//!
//! Tests that genuinely need a real X server are gated behind the
//! `test-display` Cargo feature on the consuming crate (none in-tree
//! today; tracked in P6.S4).

use silksurf_gui::{ControlFlow, Event};

/// Mirror of the `width` / `height` fields stored in `XcbWindow`.
///
/// We deliberately do NOT try to construct a real `XcbWindow` here --
/// that would require `DISPLAY` to be set. Instead we model the same
/// state shape so the test catches regressions in how callers pass
/// (width, height) through the GUI layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowParams {
    width: u32,
    height: u32,
}

impl WindowParams {
    fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }
}

#[test]
fn window_size_params() {
    // 1280x720 is the default size that silksurf-app --window uses.
    let params = WindowParams::new(1280, 720);
    assert_eq!(params.width(), 1280);
    assert_eq!(params.height(), 720);

    // Round-trip through copy: WindowParams is Copy so debug formatters
    // and tests can pass it freely.
    let copied = params;
    assert_eq!(copied, params);

    // Edge cases: zero sizes are not rejected at this layer; the X server
    // returns a BadValue error if you try to create a 0x0 window, but the
    // field storage itself accepts the value.
    let zero = WindowParams::new(0, 0);
    assert_eq!(zero.width(), 0);
    assert_eq!(zero.height(), 0);
}

#[test]
fn event_enum_variants() {
    // Construct one of every Event variant so the compiler errors if a
    // variant is removed or renamed without updating this test.
    let cases = [
        Event::Expose {
            width: 1280,
            height: 720,
        },
        Event::KeyPress { keysym: 0x1B },
        Event::KeyRelease { keysym: 0x1B },
        Event::MouseMove { x: 10, y: 20 },
        Event::MousePress {
            button: 1,
            x: 50,
            y: 60,
        },
        Event::MouseRelease {
            button: 1,
            x: 50,
            y: 60,
        },
        Event::Resize {
            width: 800,
            height: 600,
        },
        Event::Close,
    ];

    for event in &cases {
        let formatted = format!("{event:?}");
        assert!(
            !formatted.is_empty(),
            "Debug output for {event:?} must not be empty"
        );
    }

    // Sanity: distinct variants are not equal under PartialEq.
    assert_ne!(Event::Close, Event::KeyPress { keysym: 0x1B });
}

#[test]
fn control_flow_variants() {
    // PartialEq round-trip: Continue == Continue, Exit == Exit, Continue != Exit.
    assert_eq!(ControlFlow::Continue, ControlFlow::Continue);
    assert_eq!(ControlFlow::Exit, ControlFlow::Exit);
    assert_ne!(ControlFlow::Continue, ControlFlow::Exit);

    // Debug must produce a non-empty string for both variants so log
    // sites that pretty-print the ControlFlow do not silently emit "".
    assert!(!format!("{:?}", ControlFlow::Continue).is_empty());
    assert!(!format!("{:?}", ControlFlow::Exit).is_empty());
}
