//! Windowing, event loop, and platform integration (cleanroom).
//!
//! `SilkSurf` renders into a CPU-side ARGB framebuffer. This crate owns the
//! platform layer that opens a window, ships the framebuffer to the display
//! server, and pumps input events back into the app.
//!
//! `window` owns the optional XCB connection and drawable. `event_loop` owns
//! the optional synchronous wait_for_event pump. `input` owns normalized event
//! types shared by backend tests.
//!
//! All XCB calls live in `window.rs` and `event_loop.rs`; the rest of
//! the engine is XCB-free and can be tested without a display.

pub mod input;

#[cfg(feature = "xcb-backend")]
pub mod event_loop;
#[cfg(feature = "xcb-backend")]
pub mod window;

#[cfg(feature = "xcb-backend")]
pub use event_loop::EventLoop;
pub use input::{ControlFlow, Event};
#[cfg(feature = "xcb-backend")]
pub use window::XcbWindow;

#[cfg(feature = "winit-backend")]
pub mod winit_backend;
#[cfg(feature = "winit-backend")]
pub use winit_backend::{
    WinitCursorShape, WinitDamageRect, WinitDisplayBackend, WinitInput, WinitInputResult,
    WinitPresentDamage, WinitPresentedFrame, WinitRenderAction, WinitRetainedBufferTag,
    WinitRetainedBufferUpdate, WinitWakeHandle, WinitWaylandPresenter, WinitWindow,
    resolve_winit_wayland_presenter,
};
