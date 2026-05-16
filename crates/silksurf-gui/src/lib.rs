//! Windowing, event loop, and platform integration (cleanroom).
//!
//! WHY: SilkSurf renders into a CPU-side ARGB framebuffer. To make the
//! result visible on a Linux workstation we need a thin platform layer
//! that opens a window, ships the framebuffer to the display server,
//! and pumps input events back. ADR-010 records the decision to do
//! this XCB-only on Linux first; Wayland/macOS/Windows are explicit
//! future work behind the same `XcbWindow`/`EventLoop` shape.
//!
//! WHAT: Three small modules.
//!
//!   * `window`     -- `XcbWindow`: connection + drawable + present().
//!   * `event_loop` -- `EventLoop`: synchronous wait_for_event pump.
//!   * `input`      -- `Event`/`ControlFlow`: normalized event types.
//!
//! HOW: Construct an `XcbWindow`, render a frame into a `Vec<u32>`,
//! call `window.present(&pixels)`, then drive `EventLoop::run` with
//! a closure that returns `ControlFlow::Continue` until you want to
//! exit. See `silksurf-app --window` for a worked example.
//!
//! All XCB calls live in `window.rs` and `event_loop.rs`; the rest of
//! the engine is XCB-free and can be tested without a display.

pub mod event_loop;
pub mod input;
pub mod window;

pub use event_loop::EventLoop;
pub use input::{ControlFlow, Event};
pub use window::XcbWindow;

#[cfg(feature = "winit-backend")]
pub mod winit_backend;
#[cfg(feature = "winit-backend")]
pub use winit_backend::WinitWindow;
