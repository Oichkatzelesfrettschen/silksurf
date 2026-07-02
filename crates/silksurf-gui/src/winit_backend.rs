/*
 * winit_backend.rs -- cross-platform windowing via winit.
 *
 * winit 0.30 owns the platform event loop. The presenter maps the native
 * window surface as mutable 0xAARRGGBB words. RedrawRequested writes the
 * current frame into that mapped slice and presents the damaged region.
 *
 * Example:
 *   let win = WinitWindow::new("silksurf", 1280, 720).unwrap();
 *   win.run(|_w, _h, pixels| pixels.fill(0xFF6495EDu32));
 */

use std::num::NonZeroU32;
#[cfg(target_os = "linux")]
use std::os::unix::fs::FileTypeExt;
#[cfg(target_os = "linux")]
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use silksurf_core::SilkError;
use smallvec::SmallVec;
use softbuffer::{Context, Surface};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    keyboard::{Key, ModifiersState, NamedKey},
    window::{CursorIcon, Window, WindowAttributes, WindowId},
};

#[cfg(target_os = "linux")]
use winit::platform::{wayland::EventLoopBuilderExtWayland, x11::EventLoopBuilderExtX11};

#[cfg(target_os = "linux")]
#[path = "wayland_shm.rs"]
pub mod wayland_shm;
#[cfg(target_os = "linux")]
use wayland_shm::{WaylandShmDrawOutcome, WaylandShmRetainedTag, WaylandShmSurface};

type WinitSurface = Surface<Rc<Window>, Rc<Window>>;
type RenderCallback = dyn FnMut(u32, u32, u8, &mut [u32]) -> WinitPresentDamage;
type RenderReadyCallback = dyn FnMut(u32, u32) -> bool;
type RenderActionCallback = dyn FnMut(u32, u32) -> WinitRenderAction;
type RetainedUpdateCallback = dyn FnMut(u32, u32) -> Option<WinitRetainedBufferUpdate>;
type RetainedPreparedCallback = dyn FnMut(WinitRetainedBufferTag);
type PresentedCallback = dyn FnMut(WinitPresentedFrame);
type InputCallback = dyn FnMut(WinitInput, u32, u32, &WinitWakeHandle) -> WinitInputResult;
type WakeCallback = dyn FnMut() -> bool;
const MAX_PRESENT_DAMAGE_RECTS: usize = 5;
const BUFFER_WAIT_TRACE_THRESHOLD: Duration = Duration::from_millis(1);
const WAYLAND_REDRAW_PACE_INITIAL: Duration = Duration::from_millis(1);
const WAYLAND_REDRAW_PACE_COLD_BUFFER: Duration = Duration::from_millis(16);
const WAYLAND_REDRAW_PACE_DECAY_STEP: Duration = Duration::from_millis(1);
const WAYLAND_REDRAW_PACE_MIN: Duration = Duration::from_millis(1);
const WAYLAND_REDRAW_PACE_MAX: Duration = Duration::from_millis(16);
const BLOCKING_BUSY_REDRAW_RETRY_INTERVAL: Duration = Duration::from_millis(16);
const NONBLOCKING_BUSY_REDRAW_RETRY_INTERVAL: Duration = Duration::from_millis(1);
const MAX_RETAINED_UPDATES_AFTER_PRESENT: usize = 3;

#[derive(Clone, Copy, Debug)]
enum WinitUserEvent {
    Wake,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RedrawRequestKind {
    Paced,
    Urgent,
}

/// Thread-safe wake handle for background browser work.
#[derive(Clone)]
pub struct WinitWakeHandle {
    proxy: EventLoopProxy<WinitUserEvent>,
}

impl WinitWakeHandle {
    /// Wake the event loop so the app can consume ready background work.
    pub fn wake(&self) {
        let _ = self.proxy.send_event(WinitUserEvent::Wake);
    }
}

/// Browser-relevant input translated from winit events.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WinitInput {
    ScrollPixels(f32),
    CursorMoved { x: f32, y: f32 },
    PrimaryClick { x: f32, y: f32 },
    FocusAddress,
    TextInput(char),
    SubmitAddress,
    Backspace,
    Copy,
    Paste,
    Cut,
    FocusNextPageInput,
    MoveCaretLeft,
    MoveCaretRight,
    Back,
    Forward,
    Reload,
    Stop,
    PageDown,
    PageUp,
    Home,
    End,
}

/// Native cursor shape requested by browser hit testing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WinitCursorShape {
    Default,
    Pointer,
    Text,
}

impl WinitCursorShape {
    fn to_cursor_icon(self) -> CursorIcon {
        match self {
            Self::Default => CursorIcon::Default,
            Self::Pointer => CursorIcon::Pointer,
            Self::Text => CursorIcon::Text,
        }
    }
}

/// Browser input result with optional redraw and cursor updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WinitInputResult {
    pub redraw: bool,
    pub cursor: Option<WinitCursorShape>,
}

impl WinitInputResult {
    pub const fn redraw(redraw: bool) -> Self {
        Self {
            redraw,
            cursor: None,
        }
    }

    pub const fn cursor(cursor: WinitCursorShape) -> Self {
        Self {
            redraw: false,
            cursor: Some(cursor),
        }
    }
}

impl From<bool> for WinitInputResult {
    fn from(redraw: bool) -> Self {
        Self::redraw(redraw)
    }
}

/// Rectangle in native surface buffer coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WinitDamageRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Inline list of native surface damage rectangles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WinitDamageRects {
    rects: [WinitDamageRect; MAX_PRESENT_DAMAGE_RECTS],
    len: usize,
}

impl WinitDamageRects {
    fn new() -> Self {
        Self {
            rects: [WinitDamageRect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            }; MAX_PRESENT_DAMAGE_RECTS],
            len: 0,
        }
    }

    fn push(&mut self, rect: WinitDamageRect) {
        if rect.width == 0 || rect.height == 0 || self.len >= MAX_PRESENT_DAMAGE_RECTS {
            return;
        }
        self.rects[self.len] = rect;
        self.len += 1;
    }

    pub fn as_slice(&self) -> &[WinitDamageRect] {
        &self.rects[..self.len]
    }
}

/// Pixel damage returned after a redraw writes into the native presenter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WinitPresentDamage {
    Clean,
    Full,
    Rect(WinitDamageRect),
    Rects(WinitDamageRects),
}

impl WinitPresentDamage {
    /// Build rectangle damage; zero-area rectangles mean no pixel damage.
    #[must_use]
    pub fn rect(x: u32, y: u32, width: u32, height: u32) -> Self {
        if width == 0 || height == 0 {
            Self::Clean
        } else {
            Self::Rect(WinitDamageRect {
                x,
                y,
                width,
                height,
            })
        }
    }

    /// Build inline rectangle damage; zero-area rectangles are ignored.
    #[must_use]
    pub fn rects(rects: &[WinitDamageRect]) -> Self {
        let mut damage_rects = WinitDamageRects::new();
        for rect in rects {
            damage_rects.push(*rect);
        }
        match damage_rects.as_slice() {
            [] => Self::Clean,
            [rect] => Self::Rect(*rect),
            _ => Self::Rects(damage_rects),
        }
    }
}

/// Stable tag for a presenter-owned retained pixel buffer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WinitRetainedBufferTag(u64);

impl WinitRetainedBufferTag {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}

/// Retained pixel data prepared outside the input hot path.
#[derive(Debug)]
pub struct WinitRetainedBufferUpdate {
    pub tag: WinitRetainedBufferTag,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u32>,
}

/// Redraw action selected before the backend maps a presenter buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WinitRenderAction {
    Render,
    Retained {
        tag: WinitRetainedBufferTag,
        damage: WinitPresentDamage,
    },
}

/// Frame completion data reported after a presenter commits visible damage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WinitPresentedFrame {
    pub width: u32,
    pub height: u32,
    pub damage: WinitPresentDamage,
    pub retained_tag: Option<WinitRetainedBufferTag>,
}

/// Native display server selection for the winit backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WinitDisplayBackend {
    Auto,
    Wayland,
    X11,
}

impl WinitDisplayBackend {
    /// Resolve `Auto` from the current Linux desktop environment.
    #[must_use]
    pub fn resolve_for_current_environment(self) -> Self {
        #[cfg(target_os = "linux")]
        {
            resolve_display_backend(
                self,
                wayland_display_available_from_environment(),
                std::env::var_os("DISPLAY").is_some_and(|value| !value.is_empty()),
            )
        }
        #[cfg(not(target_os = "linux"))]
        {
            self
        }
    }
}

/// Native Wayland pixel presenter selection for the winit backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WinitWaylandPresenter {
    Auto,
    Shm,
    Softbuffer,
}

/// Resolve the presenter that a display backend uses at runtime.
#[must_use]
pub fn resolve_winit_wayland_presenter(
    display_backend: WinitDisplayBackend,
    wayland_presenter: WinitWaylandPresenter,
) -> WinitWaylandPresenter {
    effective_wayland_presenter(display_backend, wayland_presenter)
}

/// Cross-platform window backed by winit and a native pixel presenter.
///
/// Call `run()` to enter the event loop. The call blocks until the window
/// is closed (`CloseRequested`) or Escape is pressed.
pub struct WinitWindow {
    title: String,
    width: u32,
    height: u32,
    display_backend: WinitDisplayBackend,
    wayland_presenter: WinitWaylandPresenter,
}

impl WinitWindow {
    /// Create a `WinitWindow` with the given title and initial size.
    ///
    /// Returns `SilkError::Engine` if the platform does not support windowing
    /// (e.g., no DISPLAY on a headless CI box). The actual window is created
    /// lazily inside `run()` because winit requires an active event loop.
    pub fn new(title: &str, width: u32, height: u32) -> Result<Self, SilkError> {
        Ok(Self {
            title: title.to_string(),
            width,
            height,
            display_backend: WinitDisplayBackend::Auto,
            wayland_presenter: WinitWaylandPresenter::Auto,
        })
    }

    /// Select the native display server backend before entering the event loop.
    #[must_use]
    pub fn with_display_backend(mut self, display_backend: WinitDisplayBackend) -> Self {
        self.display_backend = display_backend;
        self
    }

    /// Select the Wayland presenter before entering the event loop.
    #[must_use]
    pub fn with_wayland_presenter(mut self, wayland_presenter: WinitWaylandPresenter) -> Self {
        self.wayland_presenter = wayland_presenter;
        self
    }

    /// Enter the event loop. Blocks until the window is closed.
    ///
    /// `render_fn(width, height, pixels)` receives the mapped presenter buffer
    /// for each redraw and writes 0xAARRGGBB pixels in place.
    pub fn run(self, mut render_fn: impl FnMut(u32, u32, &mut [u32]) + 'static) {
        self.run_with_input(
            move |width, height, pixels| {
                render_fn(width, height, pixels);
            },
            |_input, _width, _height| false,
        );
    }

    /// Enter the event loop with a browser-input callback.
    ///
    /// `input_fn(input, width, height)` returns true when the input changes
    /// visible state and requires a redraw.
    pub fn run_with_input(
        self,
        mut render_fn: impl FnMut(u32, u32, &mut [u32]) + 'static,
        mut input_fn: impl FnMut(WinitInput, u32, u32) -> bool + 'static,
    ) {
        self.run_with_input_and_wake(
            move |width, height, _buffer_age, pixels| {
                render_fn(width, height, pixels);
                WinitPresentDamage::Full
            },
            move |input, width, height, _wake| input_fn(input, width, height),
            || false,
        );
    }

    /// Enter the event loop with input and background wake callbacks.
    ///
    /// `wake_fn()` runs when `WinitWakeHandle::wake()` posts a user event from
    /// another thread. It returns true when ready work changes visible state.
    pub fn run_with_input_and_wake(
        self,
        render_fn: impl FnMut(u32, u32, u8, &mut [u32]) -> WinitPresentDamage + 'static,
        mut input_fn: impl FnMut(WinitInput, u32, u32, &WinitWakeHandle) -> bool + 'static,
        wake_fn: impl FnMut() -> bool + 'static,
    ) {
        self.run_with_input_wake_and_render_ready(
            render_fn,
            |_width, _height| true,
            move |input, width, height, wake| input_fn(input, width, height, wake).into(),
            wake_fn,
        );
    }

    /// Enter the event loop with a pre-buffer render readiness callback.
    ///
    /// `render_ready_fn(width, height)` returns false when retained state proves
    /// the redraw has no pixel changes. The backend then skips surface mapping
    /// and presentation for that event.
    pub fn run_with_input_wake_and_render_ready(
        self,
        render_fn: impl FnMut(u32, u32, u8, &mut [u32]) -> WinitPresentDamage + 'static,
        render_ready_fn: impl FnMut(u32, u32) -> bool + 'static,
        mut input_fn: impl FnMut(WinitInput, u32, u32, &WinitWakeHandle) -> WinitInputResult + 'static,
        wake_fn: impl FnMut() -> bool + 'static,
    ) {
        self.run_with_input_wake_and_render_actions(
            render_fn,
            render_ready_fn,
            |_width, _height| WinitRenderAction::Render,
            |_width, _height| None,
            |_tag| {},
            |_frame| {},
            move |input, width, height, wake| input_fn(input, width, height, wake),
            wake_fn,
        );
    }

    /// Enter the event loop with retained-presenter hooks.
    ///
    /// `render_action_fn(width, height)` can request a retained presenter buffer
    /// before the backend maps a render buffer. The backend falls back to
    /// `render_fn` when the retained buffer is unavailable.
    #[allow(clippy::too_many_arguments)]
    pub fn run_with_input_wake_and_render_actions(
        self,
        render_fn: impl FnMut(u32, u32, u8, &mut [u32]) -> WinitPresentDamage + 'static,
        render_ready_fn: impl FnMut(u32, u32) -> bool + 'static,
        render_action_fn: impl FnMut(u32, u32) -> WinitRenderAction + 'static,
        retained_update_fn: impl FnMut(u32, u32) -> Option<WinitRetainedBufferUpdate> + 'static,
        retained_prepared_fn: impl FnMut(WinitRetainedBufferTag) + 'static,
        presented_fn: impl FnMut(WinitPresentedFrame) + 'static,
        mut input_fn: impl FnMut(WinitInput, u32, u32, &WinitWakeHandle) -> WinitInputResult + 'static,
        wake_fn: impl FnMut() -> bool + 'static,
    ) {
        let mut event_loop_builder = EventLoop::<WinitUserEvent>::with_user_event();
        let resolved_display_backend = self.display_backend.resolve_for_current_environment();
        apply_display_backend(&mut event_loop_builder, resolved_display_backend);
        let event_loop = match event_loop_builder.build() {
            Ok(el) => el,
            Err(e) => {
                eprintln!("[SilkSurf] winit: cannot create event loop: {e}");
                return;
            }
        };
        event_loop.set_control_flow(ControlFlow::Wait);
        let wake_handle = WinitWakeHandle {
            proxy: event_loop.create_proxy(),
        };
        let wayland_presenter = resolve_wayland_presenter_from_environment(self.wayland_presenter);
        let mut app = WinitApp {
            title: self.title,
            init_width: self.width,
            init_height: self.height,
            window: None,
            surface: None,
            window_width: self.width,
            window_height: self.height,
            surface_width: 0,
            surface_height: 0,
            cursor_position: None,
            cursor_shape: WinitCursorShape::Default,
            modifiers: ModifiersState::empty(),
            wake_handle,
            trace_frame_timing: std::env::var_os("SILKSURF_TRACE_FRAME").is_some(),
            trace_shm_phase_timing: std::env::var_os("SILKSURF_TRACE_SHM_PHASES").is_some(),
            input_probe: WinitInputProbe::from_env(),
            display_backend: resolved_display_backend,
            wayland_presenter,
            redraw_pacing_enabled: redraw_pacing_enabled_for_backend(
                resolved_display_backend,
                wayland_presenter,
            ),
            input_redraw_bypass_pacing: input_redraw_bypass_pacing(
                resolved_display_backend,
                wayland_presenter,
            ),
            redraw_pace_interval: initial_redraw_pace_interval(
                resolved_display_backend,
                wayland_presenter,
            ),
            last_present: None,
            busy_redraw_deadline: None,
            busy_redraw_count: 0,
            busy_redraw_started_at: None,
            redraw_pending: false,
            pending_input_latency_start: None,
            render_fn: Box::new(render_fn),
            render_ready_fn: Box::new(render_ready_fn),
            render_action_fn: Box::new(render_action_fn),
            retained_update_fn: Box::new(retained_update_fn),
            retained_prepared_fn: Box::new(retained_prepared_fn),
            presented_fn: Box::new(presented_fn),
            input_fn: Box::new(move |input, width, height, wake| {
                input_fn(input, width, height, wake)
            }),
            wake_fn: Box::new(wake_fn),
        };
        if let Err(e) = event_loop.run_app(&mut app) {
            eprintln!("[SilkSurf] winit: event loop error: {e}");
        }
    }
}

#[cfg(target_os = "linux")]
fn apply_display_backend(
    event_loop_builder: &mut winit::event_loop::EventLoopBuilder<WinitUserEvent>,
    display_backend: WinitDisplayBackend,
) {
    match display_backend.resolve_for_current_environment() {
        WinitDisplayBackend::Auto => {}
        WinitDisplayBackend::Wayland => {
            event_loop_builder.with_wayland();
        }
        WinitDisplayBackend::X11 => {
            event_loop_builder.with_x11();
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn apply_display_backend(
    _event_loop_builder: &mut winit::event_loop::EventLoopBuilder<WinitUserEvent>,
    _display_backend: WinitDisplayBackend,
) {
}

#[cfg(target_os = "linux")]
fn resolve_display_backend(
    display_backend: WinitDisplayBackend,
    wayland_display_available: bool,
    display_available: bool,
) -> WinitDisplayBackend {
    match display_backend {
        WinitDisplayBackend::Auto if wayland_display_available => WinitDisplayBackend::Wayland,
        WinitDisplayBackend::Auto if display_available => WinitDisplayBackend::X11,
        _ => display_backend,
    }
}

#[cfg(target_os = "linux")]
fn wayland_display_available_from_environment() -> bool {
    let Some(wayland_display) = std::env::var_os("WAYLAND_DISPLAY") else {
        return false;
    };
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR");
    let Some(socket_path) = wayland_display_socket_path(&wayland_display, runtime_dir.as_deref())
    else {
        return false;
    };
    socket_path
        .metadata()
        .is_ok_and(|metadata| metadata.file_type().is_socket())
}

#[cfg(target_os = "linux")]
fn wayland_display_socket_path(
    wayland_display: &std::ffi::OsStr,
    runtime_dir: Option<&std::ffi::OsStr>,
) -> Option<PathBuf> {
    if wayland_display.is_empty() {
        return None;
    }
    let display_path = Path::new(wayland_display);
    if display_path.is_absolute() {
        return Some(display_path.to_path_buf());
    }
    let runtime_dir = runtime_dir?;
    if runtime_dir.is_empty() {
        return None;
    }
    Some(Path::new(runtime_dir).join(display_path))
}

fn redraw_pacing_enabled_for_backend(
    display_backend: WinitDisplayBackend,
    wayland_presenter: WinitWaylandPresenter,
) -> bool {
    display_backend == WinitDisplayBackend::Wayland
        && effective_wayland_presenter(display_backend, wayland_presenter)
            == WinitWaylandPresenter::Softbuffer
}

fn input_redraw_bypass_pacing(
    _display_backend: WinitDisplayBackend,
    _wayland_presenter: WinitWaylandPresenter,
) -> bool {
    true
}

fn initial_redraw_pace_interval(
    display_backend: WinitDisplayBackend,
    wayland_presenter: WinitWaylandPresenter,
) -> Duration {
    if redraw_pacing_enabled_for_backend(display_backend, wayland_presenter) {
        WAYLAND_REDRAW_PACE_INITIAL
    } else {
        Duration::ZERO
    }
}

fn next_redraw_pace_interval(
    current: Duration,
    buffer_elapsed: Duration,
    buffer_age: u8,
) -> Duration {
    if buffer_elapsed >= BUFFER_WAIT_TRACE_THRESHOLD {
        let doubled = current.saturating_mul(2);
        let measured = buffer_elapsed.saturating_mul(2);
        return doubled.max(measured).min(WAYLAND_REDRAW_PACE_MAX);
    }
    if buffer_age == 0 {
        return current.max(WAYLAND_REDRAW_PACE_COLD_BUFFER);
    }
    if buffer_elapsed <= Duration::from_micros(100) {
        return current
            .saturating_sub(WAYLAND_REDRAW_PACE_DECAY_STEP)
            .max(WAYLAND_REDRAW_PACE_MIN);
    }
    current
}

fn effective_wayland_presenter(
    display_backend: WinitDisplayBackend,
    wayland_presenter: WinitWaylandPresenter,
) -> WinitWaylandPresenter {
    if display_backend != WinitDisplayBackend::Wayland {
        return WinitWaylandPresenter::Softbuffer;
    }
    match wayland_presenter {
        WinitWaylandPresenter::Auto => WinitWaylandPresenter::Shm,
        explicit => explicit,
    }
}

fn wayland_shm_failure_uses_softbuffer_fallback(wayland_presenter: WinitWaylandPresenter) -> bool {
    wayland_presenter == WinitWaylandPresenter::Auto
}

fn resolve_wayland_presenter_from_environment(
    configured: WinitWaylandPresenter,
) -> WinitWaylandPresenter {
    match std::env::var("SILKSURF_WAYLAND_PRESENTER").ok().as_deref() {
        Some("auto") => WinitWaylandPresenter::Auto,
        Some("shm") => WinitWaylandPresenter::Shm,
        Some("softbuffer") => WinitWaylandPresenter::Softbuffer,
        Some(other) => {
            eprintln!(
                "[SilkSurf] unknown SILKSURF_WAYLAND_PRESENTER value: {other}; using {configured:?}"
            );
            configured
        }
        None if std::env::var_os("SILKSURF_EXPERIMENTAL_WAYLAND_SHM").is_some() => {
            WinitWaylandPresenter::Shm
        }
        None => configured,
    }
}

fn translate_keyboard_input(key_event: &KeyEvent, modifiers: ModifiersState) -> Option<WinitInput> {
    translate_logical_key(&key_event.logical_key.as_ref(), modifiers)
}

fn translate_logical_key(key: &Key<&str>, modifiers: ModifiersState) -> Option<WinitInput> {
    let command_modifier = modifiers.control_key() || modifiers.super_key();
    match key {
        Key::Character(ch) if command_modifier && ch.eq_ignore_ascii_case("l") => {
            Some(WinitInput::FocusAddress)
        }
        Key::Character(ch) if command_modifier && ch.eq_ignore_ascii_case("c") => {
            Some(WinitInput::Copy)
        }
        Key::Character(ch) if command_modifier && ch.eq_ignore_ascii_case("v") => {
            Some(WinitInput::Paste)
        }
        Key::Character(ch) if command_modifier && ch.eq_ignore_ascii_case("x") => {
            Some(WinitInput::Cut)
        }
        Key::Named(NamedKey::BrowserBack | NamedKey::GoBack) => Some(WinitInput::Back),
        Key::Named(NamedKey::BrowserForward) => Some(WinitInput::Forward),
        Key::Named(NamedKey::BrowserRefresh | NamedKey::F5) => Some(WinitInput::Reload),
        Key::Named(NamedKey::BrowserStop | NamedKey::Cancel) => Some(WinitInput::Stop),
        Key::Named(NamedKey::Enter) => Some(WinitInput::SubmitAddress),
        Key::Named(NamedKey::Backspace) => Some(WinitInput::Backspace),
        Key::Named(NamedKey::Tab) => Some(WinitInput::FocusNextPageInput),
        Key::Named(NamedKey::PageDown) => Some(WinitInput::PageDown),
        Key::Named(NamedKey::PageUp) => Some(WinitInput::PageUp),
        Key::Named(NamedKey::Home) => Some(WinitInput::Home),
        Key::Named(NamedKey::End) => Some(WinitInput::End),
        Key::Named(NamedKey::ArrowLeft) => Some(WinitInput::MoveCaretLeft),
        Key::Named(NamedKey::ArrowRight) => Some(WinitInput::MoveCaretRight),
        Key::Named(NamedKey::ArrowDown) => Some(WinitInput::ScrollPixels(48.0)),
        Key::Named(NamedKey::ArrowUp) => Some(WinitInput::ScrollPixels(-48.0)),
        Key::Character(ch) if !command_modifier && !modifiers.alt_key() => ch
            .chars()
            .next()
            .filter(|c| c.is_ascii_graphic() || *c == ' ')
            .map(WinitInput::TextInput),
        _ => None,
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
    surface: Option<WinitSurfaceKind>,
    window_width: u32,
    window_height: u32,
    surface_width: u32,
    surface_height: u32,
    cursor_position: Option<(f32, f32)>,
    cursor_shape: WinitCursorShape,
    modifiers: ModifiersState,
    wake_handle: WinitWakeHandle,
    trace_frame_timing: bool,
    trace_shm_phase_timing: bool,
    input_probe: Option<WinitInputProbe>,
    display_backend: WinitDisplayBackend,
    wayland_presenter: WinitWaylandPresenter,
    redraw_pacing_enabled: bool,
    input_redraw_bypass_pacing: bool,
    redraw_pace_interval: Duration,
    last_present: Option<Instant>,
    busy_redraw_deadline: Option<Instant>,
    busy_redraw_count: u32,
    busy_redraw_started_at: Option<Instant>,
    redraw_pending: bool,
    pending_input_latency_start: Option<Instant>,
    render_fn: Box<RenderCallback>,
    render_ready_fn: Box<RenderReadyCallback>,
    render_action_fn: Box<RenderActionCallback>,
    retained_update_fn: Box<RetainedUpdateCallback>,
    retained_prepared_fn: Box<RetainedPreparedCallback>,
    presented_fn: Box<PresentedCallback>,
    input_fn: Box<InputCallback>,
    wake_fn: Box<WakeCallback>,
}

impl ApplicationHandler<WinitUserEvent> for WinitApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::Wait);
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

        let mut surface =
            match create_surface_kind(window.clone(), self.display_backend, self.wayland_presenter)
            {
                Ok(surface) => surface,
                Err(e) => {
                    eprintln!("[SilkSurf] surface: {e}");
                    event_loop.exit();
                    return;
                }
            };
        let _ = resize_surface_kind(
            &mut surface,
            &mut self.surface_width,
            &mut self.surface_height,
            self.init_width,
            self.init_height,
        );

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
                self.handle_keyboard_input(event_loop, &key_event);
            }

            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }

            WindowEvent::MouseWheel { delta, .. } => {
                self.handle_mouse_wheel(delta);
            }

            WindowEvent::CursorMoved { position, .. } => {
                let x = position.x as f32;
                let y = position.y as f32;
                self.cursor_position = Some((x, y));
                self.handle_input(WinitInput::CursorMoved { x, y });
            }

            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                self.handle_primary_click();
            }

            WindowEvent::RedrawRequested => {
                self.handle_redraw_requested(event_loop);
            }

            WindowEvent::Resized(size) => {
                self.handle_resize(size);
            }

            _ => {}
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: WinitUserEvent) {
        match event {
            WinitUserEvent::Wake => {
                self.handle_ready_work();
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.handle_ready_work();
        self.drive_input_probe();
        self.flush_paced_redraw(event_loop);
    }
}

impl WinitApp {
    fn handle_keyboard_input(&mut self, event_loop: &ActiveEventLoop, key_event: &KeyEvent) {
        if key_event.logical_key == Key::Named(NamedKey::Escape) {
            event_loop.exit();
            return;
        }
        if key_event.state != ElementState::Pressed {
            return;
        }
        if let Some(input) = translate_keyboard_input(key_event, self.modifiers) {
            self.handle_input(input);
        }
    }

    fn handle_mouse_wheel(&mut self, delta: MouseScrollDelta) {
        let scroll_pixels = match delta {
            MouseScrollDelta::LineDelta(_, y) => -y * 48.0,
            MouseScrollDelta::PixelDelta(pos) => -pos.y as f32,
        };
        if scroll_pixels.abs() > f32::EPSILON {
            self.handle_input(WinitInput::ScrollPixels(scroll_pixels));
        }
    }

    fn handle_primary_click(&mut self) {
        if let Some((x, y)) = self.cursor_position {
            self.handle_input(WinitInput::PrimaryClick { x, y });
        }
    }

    fn handle_redraw_requested(&mut self, event_loop: &ActiveEventLoop) {
        let redraw_start = Instant::now();
        let (width, height) = self.window_size();
        if !(self.render_ready_fn)(width, height) {
            self.trace_clean_redraw_skip(width, height, redraw_start);
            return;
        }
        let render_action = (self.render_action_fn)(width, height);

        let mut frame_presented = false;
        if let Some(mut surface) = self.surface.take() {
            if resize_surface_kind(
                &mut surface,
                &mut self.surface_width,
                &mut self.surface_height,
                width,
                height,
            ) {
                frame_presented = self.draw_resized_surface(
                    event_loop,
                    &mut surface,
                    width,
                    height,
                    redraw_start,
                    self.pending_input_latency_start,
                    render_action,
                );
            }
            if frame_presented {
                self.pending_input_latency_start = None;
                self.warm_surface_after_present(&mut surface);
                self.prepare_retained_buffer_after_present(&mut surface, width, height);
                self.mark_probe_frame_presented(event_loop);
            }
            self.surface = Some(surface);
        }
    }

    fn trace_clean_redraw_skip(&self, width: u32, height: u32, redraw_start: Instant) {
        if self.trace_frame_timing {
            eprintln!(
                "[SilkSurf] frame: {width}x{height} skipped clean redraw, total {:?}",
                redraw_start.elapsed()
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_resized_surface(
        &mut self,
        event_loop: &ActiveEventLoop,
        surface: &mut WinitSurfaceKind,
        width: u32,
        height: u32,
        redraw_start: Instant,
        input_latency_start: Option<Instant>,
        render_action: WinitRenderAction,
    ) -> bool {
        match draw_surface_kind(
            surface,
            width,
            height,
            self.trace_frame_timing,
            self.trace_shm_phase_timing,
            redraw_start,
            input_latency_start,
            render_action,
            &mut self.render_fn,
        ) {
            Ok(DrawSurfaceOutcome::Presented {
                damage,
                buffer_elapsed,
                buffer_age,
                retained_tag,
            }) => self.handle_presented_frame(
                width,
                height,
                damage,
                buffer_elapsed,
                buffer_age,
                retained_tag,
            ),
            Ok(DrawSurfaceOutcome::Busy) => {
                self.handle_busy_surface(event_loop);
                false
            }
            Err(e) => {
                eprintln!("[SilkSurf] surface: {e}");
                false
            }
        }
    }

    fn warm_surface_after_present(&self, surface: &mut WinitSurfaceKind) {
        let warm_start = Instant::now();
        if !warm_surface_kind_after_present(surface) {
            return;
        }
        if self.trace_shm_phase_timing {
            eprintln!(
                "[SilkSurf] shm phases: idle_seed {:?}",
                warm_start.elapsed()
            );
        }
    }

    fn prepare_retained_buffer_after_present(
        &mut self,
        surface: &mut WinitSurfaceKind,
        width: u32,
        height: u32,
    ) {
        for _ in 0..MAX_RETAINED_UPDATES_AFTER_PRESENT {
            let Some(update) = (self.retained_update_fn)(width, height) else {
                return;
            };
            let Ok(wrote) = write_retained_surface_kind(surface, &update) else {
                return;
            };
            if !wrote {
                return;
            }
            (self.retained_prepared_fn)(update.tag);
        }
    }

    fn handle_presented_frame(
        &mut self,
        width: u32,
        height: u32,
        damage: WinitPresentDamage,
        buffer_elapsed: Duration,
        buffer_age: u8,
        retained_tag: Option<WinitRetainedBufferTag>,
    ) -> bool {
        if damage == WinitPresentDamage::Clean {
            return false;
        }
        self.observe_redraw_buffer_elapsed(buffer_elapsed, buffer_age);
        self.last_present = Some(Instant::now());
        self.busy_redraw_deadline = None;
        self.trace_and_clear_busy_redraws();
        (self.presented_fn)(WinitPresentedFrame {
            width,
            height,
            damage,
            retained_tag,
        });
        true
    }

    fn handle_busy_surface(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let retry_deadline = now + self.busy_redraw_retry_interval();
        self.redraw_pending = true;
        self.busy_redraw_deadline = Some(retry_deadline);
        self.busy_redraw_count = self.busy_redraw_count.saturating_add(1);
        if self.busy_redraw_started_at.is_none() {
            self.busy_redraw_started_at = Some(now);
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(retry_deadline));
        if self.trace_frame_timing && self.busy_redraw_count == 1 {
            eprintln!("[SilkSurf] frame: Wayland buffers busy; retry at {retry_deadline:?}");
        }
    }

    fn trace_and_clear_busy_redraws(&mut self) {
        let retry_count = self.busy_redraw_count;
        if retry_count == 0 {
            return;
        }
        if self.trace_frame_timing {
            if let Some(started_at) = self.busy_redraw_started_at {
                eprintln!(
                    "[SilkSurf] frame: Wayland buffers released after {retry_count} retries over {:?}",
                    started_at.elapsed()
                );
            } else {
                eprintln!("[SilkSurf] frame: Wayland buffers released after {retry_count} retries");
            }
        }
        self.busy_redraw_count = 0;
        self.busy_redraw_started_at = None;
    }

    fn busy_redraw_retry_interval(&self) -> Duration {
        if effective_wayland_presenter(self.display_backend, self.wayland_presenter)
            == WinitWaylandPresenter::Shm
        {
            NONBLOCKING_BUSY_REDRAW_RETRY_INTERVAL
        } else {
            BLOCKING_BUSY_REDRAW_RETRY_INTERVAL
        }
    }

    fn handle_resize(&mut self, size: PhysicalSize<u32>) {
        self.window_width = size.width;
        self.window_height = size.height;
        if self.trace_frame_timing {
            eprintln!("[SilkSurf] resize: {}x{}", size.width, size.height);
        }
        if let Some(surface) = &mut self.surface {
            let _ = resize_surface_kind(
                surface,
                &mut self.surface_width,
                &mut self.surface_height,
                size.width,
                size.height,
            );
        }
        if let Some(window) = self.window.clone() {
            self.request_or_pace_redraw(&window, RedrawRequestKind::Paced);
        }
    }

    fn handle_ready_work(&mut self) {
        if (self.wake_fn)()
            && let Some(window) = self.window.clone()
        {
            self.request_or_pace_redraw(&window, RedrawRequestKind::Paced);
        }
    }

    fn handle_input(&mut self, input: WinitInput) -> bool {
        let input_latency_start = Instant::now();
        let (width, height) = self.window_size();
        let result = (self.input_fn)(input, width, height, &self.wake_handle);
        if let Some(window) = self.window.clone() {
            self.apply_cursor_shape(&window, result.cursor);
            if result.redraw {
                self.pending_input_latency_start = Some(input_latency_start);
                self.request_or_pace_redraw(&window, RedrawRequestKind::Urgent);
            }
        }
        result.redraw
    }

    fn apply_cursor_shape(&mut self, window: &Window, cursor: Option<WinitCursorShape>) {
        let Some(cursor) = cursor else {
            return;
        };
        if self.cursor_shape == cursor {
            return;
        }
        self.cursor_shape = cursor;
        window.set_cursor(cursor.to_cursor_icon());
    }

    fn request_or_pace_redraw(&mut self, window: &Window, kind: RedrawRequestKind) {
        let now = Instant::now();
        if let Some(deadline) = redraw_request_wait_deadline(
            self.paced_redraw_deadline(),
            now,
            kind,
            self.input_redraw_bypass_pacing,
        ) {
            self.redraw_pending = true;
            if self.trace_frame_timing {
                eprintln!("[SilkSurf] redraw paced until {deadline:?}");
            }
            return;
        }
        self.redraw_pending = false;
        window.request_redraw();
    }

    fn observe_redraw_buffer_elapsed(&mut self, buffer_elapsed: Duration, buffer_age: u8) {
        if !self.redraw_pacing_enabled {
            return;
        }
        let next_interval =
            next_redraw_pace_interval(self.redraw_pace_interval, buffer_elapsed, buffer_age);
        if self.trace_frame_timing && next_interval != self.redraw_pace_interval {
            eprintln!(
                "[SilkSurf] redraw pace adjusted {:?} -> {:?} after buffer {:?}, age {buffer_age}",
                self.redraw_pace_interval, next_interval, buffer_elapsed
            );
        }
        self.redraw_pace_interval = next_interval;
    }

    fn flush_paced_redraw(&mut self, event_loop: &ActiveEventLoop) {
        if !self.redraw_pending {
            return;
        }
        let Some(window) = self.window.clone() else {
            return;
        };
        let now = Instant::now();
        if let Some(deadline) =
            redraw_flush_wait_deadline(self.busy_redraw_deadline, self.paced_redraw_deadline(), now)
        {
            event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
            return;
        }
        if self
            .busy_redraw_deadline
            .is_some_and(|deadline| now >= deadline)
        {
            self.busy_redraw_deadline = None;
        }
        self.redraw_pending = false;
        event_loop.set_control_flow(ControlFlow::Wait);
        window.request_redraw();
    }

    fn paced_redraw_deadline(&self) -> Option<Instant> {
        if self.redraw_pacing_enabled {
            self.last_present
                .map(|last_present| last_present + self.redraw_pace_interval)
        } else {
            None
        }
    }

    fn mark_probe_frame_presented(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(probe) = &mut self.input_probe {
            probe.arm_next_input();
            if probe.exit_after_finish && probe.finished() && probe.exit_delay_elapsed() {
                event_loop.exit();
            }
        }
    }

    fn drive_input_probe(&mut self) {
        while let Some(input) = self
            .input_probe
            .as_ref()
            .and_then(WinitInputProbe::next_input)
        {
            if self.trace_frame_timing {
                eprintln!("[SilkSurf] probe input: {input:?}");
            }
            if !self.handle_input(input) {
                return;
            }
            if let Some(probe) = &mut self.input_probe {
                probe.mark_input_dispatched();
            }
        }
    }

    fn window_size(&self) -> (u32, u32) {
        (self.window_width, self.window_height)
    }
}

fn redraw_request_wait_deadline(
    deadline: Option<Instant>,
    now: Instant,
    kind: RedrawRequestKind,
    input_bypasses_pacing: bool,
) -> Option<Instant> {
    match (kind, deadline) {
        (RedrawRequestKind::Paced, Some(deadline)) if now < deadline => Some(deadline),
        (RedrawRequestKind::Urgent, Some(deadline)) if now < deadline && !input_bypasses_pacing => {
            Some(deadline)
        }
        _ => None,
    }
}

fn redraw_flush_wait_deadline(
    busy_deadline: Option<Instant>,
    paced_deadline: Option<Instant>,
    now: Instant,
) -> Option<Instant> {
    busy_deadline
        .filter(|deadline| now < *deadline)
        .or_else(|| paced_deadline.filter(|deadline| now < *deadline))
}

const ADDRESS_INPUT_PROBE_STEPS: &[WinitInput] = &[
    WinitInput::FocusAddress,
    WinitInput::TextInput('z'),
    WinitInput::TextInput('z'),
];
const ADDRESS_CARET_PROBE_STEPS: &[WinitInput] = &[
    WinitInput::FocusAddress,
    WinitInput::TextInput('a'),
    WinitInput::TextInput('c'),
    WinitInput::MoveCaretLeft,
    WinitInput::TextInput('b'),
];
const CHROME_INPUT_PROBE_STEPS: &[WinitInput] = &[
    WinitInput::PrimaryClick { x: 55.0, y: 22.0 },
    WinitInput::PrimaryClick { x: 15.0, y: 22.0 },
    WinitInput::PrimaryClick { x: 35.0, y: 22.0 },
    WinitInput::PrimaryClick { x: 75.0, y: 22.0 },
    WinitInput::PrimaryClick { x: 15.0, y: 22.0 },
];
const PAGE_INPUT_PROBE_STEPS: &[WinitInput] =
    &[WinitInput::FocusNextPageInput, WinitInput::TextInput('!')];
const FORM_SUBMIT_PROBE_STEPS: &[WinitInput] = &[
    WinitInput::FocusNextPageInput,
    WinitInput::TextInput('!'),
    WinitInput::SubmitAddress,
];
const RELOAD_INPUT_PROBE_STEPS: &[WinitInput] = &[WinitInput::Reload];
const SCROLL_INPUT_PROBE_STEPS: &[WinitInput] = &[
    WinitInput::ScrollPixels(96.0),
    WinitInput::ScrollPixels(-48.0),
];
const HOVER_INPUT_PROBE_STEPS: &[WinitInput] = &[
    WinitInput::CursorMoved { x: 48.0, y: 220.0 },
    WinitInput::CursorMoved { x: 420.0, y: 420.0 },
];

struct WinitInputProbe {
    steps: Vec<WinitInput>,
    wait_after_step: Vec<bool>,
    next_index: usize,
    waiting_for_redraw: bool,
    armed: bool,
    exit_after_finish: bool,
    exit_frame_delay: usize,
}

impl WinitInputProbe {
    fn from_env() -> Option<Self> {
        match std::env::var("SILKSURF_PROBE_INPUT").ok().as_deref() {
            Some("smoke") => Some(Self {
                steps: Vec::new(),
                wait_after_step: Vec::new(),
                next_index: 0,
                waiting_for_redraw: true,
                armed: false,
                exit_after_finish: probe_exit_after_finish_enabled(),
                exit_frame_delay: 0,
            }),
            Some("address") => Some(Self {
                steps: ADDRESS_INPUT_PROBE_STEPS.to_vec(),
                wait_after_step: wait_after_each_step(ADDRESS_INPUT_PROBE_STEPS.len()),
                next_index: 0,
                waiting_for_redraw: true,
                armed: false,
                exit_after_finish: probe_exit_after_finish_enabled(),
                exit_frame_delay: 0,
            }),
            Some("address-caret") => Some(Self {
                steps: ADDRESS_CARET_PROBE_STEPS.to_vec(),
                wait_after_step: address_caret_probe_wait_steps(),
                next_index: 0,
                waiting_for_redraw: true,
                armed: false,
                exit_after_finish: probe_exit_after_finish_enabled(),
                exit_frame_delay: 0,
            }),
            Some("chrome") => Some(Self {
                steps: CHROME_INPUT_PROBE_STEPS.to_vec(),
                wait_after_step: wait_after_each_step(CHROME_INPUT_PROBE_STEPS.len()),
                next_index: 0,
                waiting_for_redraw: true,
                armed: false,
                exit_after_finish: probe_exit_after_finish_enabled(),
                exit_frame_delay: 0,
            }),
            Some("hover") => Some(Self {
                steps: HOVER_INPUT_PROBE_STEPS.to_vec(),
                wait_after_step: wait_after_each_step(HOVER_INPUT_PROBE_STEPS.len()),
                next_index: 0,
                waiting_for_redraw: true,
                armed: false,
                exit_after_finish: probe_exit_after_finish_enabled(),
                exit_frame_delay: 0,
            }),
            Some("page-input") => Some(Self {
                steps: PAGE_INPUT_PROBE_STEPS.to_vec(),
                wait_after_step: wait_after_each_step(PAGE_INPUT_PROBE_STEPS.len()),
                next_index: 0,
                waiting_for_redraw: true,
                armed: false,
                exit_after_finish: probe_exit_after_finish_enabled(),
                exit_frame_delay: 0,
            }),
            Some("runtime-text") => Some(Self {
                steps: Vec::new(),
                wait_after_step: Vec::new(),
                next_index: 0,
                waiting_for_redraw: true,
                armed: false,
                exit_after_finish: probe_exit_after_finish_enabled(),
                exit_frame_delay: 1,
            }),
            Some("form-submit") => Some(Self {
                steps: FORM_SUBMIT_PROBE_STEPS.to_vec(),
                wait_after_step: wait_after_each_step(FORM_SUBMIT_PROBE_STEPS.len()),
                next_index: 0,
                waiting_for_redraw: true,
                armed: false,
                exit_after_finish: probe_exit_after_finish_enabled(),
                exit_frame_delay: 1,
            }),
            Some("reload") => Some(Self {
                steps: RELOAD_INPUT_PROBE_STEPS.to_vec(),
                wait_after_step: wait_after_each_step(RELOAD_INPUT_PROBE_STEPS.len()),
                next_index: 0,
                waiting_for_redraw: true,
                armed: false,
                exit_after_finish: probe_exit_after_finish_enabled(),
                exit_frame_delay: 1,
            }),
            Some("scroll") => Some(Self {
                steps: SCROLL_INPUT_PROBE_STEPS.to_vec(),
                wait_after_step: wait_after_each_step(SCROLL_INPUT_PROBE_STEPS.len()),
                next_index: 0,
                waiting_for_redraw: true,
                armed: false,
                exit_after_finish: probe_exit_after_finish_enabled(),
                exit_frame_delay: 0,
            }),
            Some("stop") => build_stop_probe_steps().map(|(steps, wait_after_step)| Self {
                steps,
                wait_after_step,
                next_index: 0,
                waiting_for_redraw: true,
                armed: false,
                exit_after_finish: probe_exit_after_finish_enabled(),
                exit_frame_delay: 0,
            }),
            Some(other) => {
                eprintln!("[SilkSurf] unknown SILKSURF_PROBE_INPUT value: {other}");
                None
            }
            None => None,
        }
    }

    fn arm_next_input(&mut self) {
        self.armed = true;
        self.waiting_for_redraw = false;
    }

    fn next_input(&self) -> Option<WinitInput> {
        if !self.armed || self.waiting_for_redraw {
            return None;
        }
        self.steps.get(self.next_index).copied()
    }

    fn mark_input_dispatched(&mut self) {
        let wait_for_redraw = self
            .wait_after_step
            .get(self.next_index)
            .copied()
            .unwrap_or(true);
        self.next_index += 1;
        self.waiting_for_redraw = wait_for_redraw;
    }

    fn finished(&self) -> bool {
        self.next_index >= self.steps.len() && !self.waiting_for_redraw
    }

    fn exit_delay_elapsed(&mut self) -> bool {
        if self.exit_frame_delay == 0 {
            return true;
        }
        self.exit_frame_delay -= 1;
        false
    }
}

fn build_stop_probe_steps() -> Option<(Vec<WinitInput>, Vec<bool>)> {
    let target = std::env::var("SILKSURF_PROBE_NAVIGATE_URL").ok()?;
    Some(build_stop_probe_steps_for_target(&target))
}

fn build_stop_probe_steps_for_target(target: &str) -> (Vec<WinitInput>, Vec<bool>) {
    let mut steps = Vec::with_capacity(target.len() + 3);
    let mut wait_after_step = Vec::with_capacity(target.len() + 3);
    steps.push(WinitInput::FocusAddress);
    wait_after_step.push(true);
    for ch in target.chars() {
        steps.push(WinitInput::TextInput(ch));
        wait_after_step.push(false);
    }
    steps.push(WinitInput::SubmitAddress);
    wait_after_step.push(true);
    steps.push(WinitInput::PrimaryClick { x: 95.0, y: 22.0 });
    wait_after_step.push(true);
    (steps, wait_after_step)
}

fn wait_after_each_step(len: usize) -> Vec<bool> {
    vec![true; len]
}

fn address_caret_probe_wait_steps() -> Vec<bool> {
    vec![false, false, false, false, true]
}

fn probe_exit_after_finish_enabled() -> bool {
    std::env::var_os("SILKSURF_PROBE_EXIT_AFTER_INPUT").is_some()
}

fn softbuffer_damage_rect(
    rect: WinitDamageRect,
    surface_width: u32,
    surface_height: u32,
) -> Option<softbuffer::Rect> {
    let x = rect.x.min(surface_width);
    let y = rect.y.min(surface_height);
    let x_end = rect.x.saturating_add(rect.width).min(surface_width);
    let y_end = rect.y.saturating_add(rect.height).min(surface_height);
    Some(softbuffer::Rect {
        x,
        y,
        width: NonZeroU32::new(x_end.checked_sub(x)?)?,
        height: NonZeroU32::new(y_end.checked_sub(y)?)?,
    })
}

#[allow(clippy::large_enum_variant)]
enum WinitSurfaceKind {
    Softbuffer(WinitSurface),
    #[cfg(target_os = "linux")]
    WaylandShm(WaylandShmSurface),
}

enum DrawSurfaceOutcome {
    Presented {
        damage: WinitPresentDamage,
        buffer_elapsed: Duration,
        buffer_age: u8,
        retained_tag: Option<WinitRetainedBufferTag>,
    },
    Busy,
}

fn create_surface_kind(
    window: Rc<Window>,
    display_backend: WinitDisplayBackend,
    wayland_presenter: WinitWaylandPresenter,
) -> Result<WinitSurfaceKind, String> {
    #[cfg(target_os = "linux")]
    if effective_wayland_presenter(display_backend, wayland_presenter) == WinitWaylandPresenter::Shm
    {
        match WaylandShmSurface::new(window.clone()) {
            Ok(surface) => return Ok(WinitSurfaceKind::WaylandShm(surface)),
            Err(err) if wayland_shm_failure_uses_softbuffer_fallback(wayland_presenter) => {
                eprintln!("[SilkSurf] Wayland SHM presenter unavailable: {err}; using softbuffer");
            }
            Err(err) => return Err(err),
        }
    }

    let context = Context::new(window.clone()).map_err(|e| format!("softbuffer context: {e}"))?;
    Surface::new(&context, window)
        .map(WinitSurfaceKind::Softbuffer)
        .map_err(|e| format!("softbuffer surface: {e}"))
}

#[allow(clippy::too_many_arguments)]
fn draw_surface_kind(
    surface: &mut WinitSurfaceKind,
    width: u32,
    height: u32,
    trace_frame_timing: bool,
    trace_shm_phase_timing: bool,
    redraw_start: Instant,
    input_latency_start: Option<Instant>,
    render_action: WinitRenderAction,
    render_fn: &mut Box<RenderCallback>,
) -> Result<DrawSurfaceOutcome, String> {
    match surface {
        WinitSurfaceKind::Softbuffer(surface) => draw_softbuffer_surface(
            surface,
            width,
            height,
            trace_frame_timing,
            redraw_start,
            input_latency_start,
            render_action,
            render_fn,
        ),
        #[cfg(target_os = "linux")]
        WinitSurfaceKind::WaylandShm(surface) => draw_wayland_shm_surface(
            surface,
            width,
            height,
            trace_frame_timing,
            trace_shm_phase_timing,
            redraw_start,
            input_latency_start,
            render_action,
            render_fn,
        ),
    }
}

fn warm_surface_kind_after_present(surface: &mut WinitSurfaceKind) -> bool {
    match surface {
        WinitSurfaceKind::Softbuffer(_) => false,
        #[cfg(target_os = "linux")]
        WinitSurfaceKind::WaylandShm(surface) => surface.warm_one_released_virgin_buffer(),
    }
}

fn write_retained_surface_kind(
    surface: &mut WinitSurfaceKind,
    update: &WinitRetainedBufferUpdate,
) -> Result<bool, String> {
    match surface {
        WinitSurfaceKind::Softbuffer(_) => Ok(false),
        #[cfg(target_os = "linux")]
        WinitSurfaceKind::WaylandShm(surface) => surface.write_released_retained_buffer(
            WaylandShmRetainedTag::new(update.tag.value()),
            update.width,
            update.height,
            &update.pixels,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_softbuffer_surface(
    surface: &mut WinitSurface,
    width: u32,
    height: u32,
    trace_frame_timing: bool,
    redraw_start: Instant,
    input_latency_start: Option<Instant>,
    _render_action: WinitRenderAction,
    render_fn: &mut Box<RenderCallback>,
) -> Result<DrawSurfaceOutcome, String> {
    let buffer_start = Instant::now();
    let mut buf = surface
        .buffer_mut()
        .map_err(|e| format!("softbuffer buffer acquisition failed: {e}"))?;
    let buffer_age = buf.age();
    let buffer_elapsed = buffer_start.elapsed();
    if trace_frame_timing && buffer_elapsed >= BUFFER_WAIT_TRACE_THRESHOLD {
        eprintln!("[SilkSurf] frame wait: buffer_mut blocked {buffer_elapsed:?}, age {buffer_age}");
    }
    let render_start = Instant::now();
    let present_damage = {
        let pixels: &mut [u32] = &mut buf;
        render_fn(width, height, buffer_age, pixels)
    };
    let render_elapsed = render_start.elapsed();
    let present_start = Instant::now();
    match present_damage {
        WinitPresentDamage::Clean => {}
        WinitPresentDamage::Full => buf
            .present()
            .map_err(|e| format!("softbuffer present error: {e}"))?,
        WinitPresentDamage::Rect(rect) => {
            if let Some(rect) = softbuffer_damage_rect(rect, width, height) {
                buf.present_with_damage(&[rect])
                    .map_err(|e| format!("softbuffer present error: {e}"))?;
            }
        }
        WinitPresentDamage::Rects(rects) => {
            let mut softbuffer_rects =
                SmallVec::<[softbuffer::Rect; MAX_PRESENT_DAMAGE_RECTS]>::new();
            for rect in rects.as_slice() {
                if let Some(rect) = softbuffer_damage_rect(*rect, width, height) {
                    softbuffer_rects.push(rect);
                }
            }
            if !softbuffer_rects.is_empty() {
                buf.present_with_damage(&softbuffer_rects)
                    .map_err(|e| format!("softbuffer present error: {e}"))?;
            }
        }
    }
    let present_elapsed = present_start.elapsed();
    if trace_frame_timing {
        let draw_elapsed = buffer_start.elapsed();
        let predraw_elapsed = buffer_start.duration_since(redraw_start);
        let input_to_present_elapsed =
            input_latency_start.map_or(Duration::ZERO, |start| start.elapsed());
        eprintln!(
            "[SilkSurf] frame: {width}x{height} age {buffer_age}, damage {present_damage:?}, input_to_present {input_to_present_elapsed:?}, predraw {predraw_elapsed:?}, buffer {buffer_elapsed:?}, render {render_elapsed:?}, present {present_elapsed:?}, draw {draw_elapsed:?}, total {:?}",
            redraw_start.elapsed()
        );
    }
    Ok(DrawSurfaceOutcome::Presented {
        damage: present_damage,
        buffer_elapsed,
        buffer_age,
        retained_tag: None,
    })
}

#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)]
fn draw_wayland_shm_surface(
    surface: &mut WaylandShmSurface,
    width: u32,
    height: u32,
    trace_frame_timing: bool,
    trace_shm_phase_timing: bool,
    redraw_start: Instant,
    input_latency_start: Option<Instant>,
    render_action: WinitRenderAction,
    render_fn: &mut Box<RenderCallback>,
) -> Result<DrawSurfaceOutcome, String> {
    let buffer_start = Instant::now();
    let mut render_elapsed = Duration::ZERO;
    let mut retained_tag = None;
    let outcome = match render_action {
        WinitRenderAction::Retained { tag, damage } => {
            match surface.present_retained_buffer(
                WaylandShmRetainedTag::new(tag.value()),
                damage,
                trace_shm_phase_timing,
            )? {
                Some(outcome) => {
                    retained_tag = Some(tag);
                    outcome
                }
                None => surface.draw_and_present(
                    width,
                    height,
                    trace_shm_phase_timing,
                    |buffer_age, pixels| {
                        let render_start = Instant::now();
                        let damage = render_fn(width, height, buffer_age, pixels);
                        render_elapsed = render_start.elapsed();
                        damage
                    },
                )?,
            }
        }
        WinitRenderAction::Render => surface.draw_and_present(
            width,
            height,
            trace_shm_phase_timing,
            |buffer_age, pixels| {
                let render_start = Instant::now();
                let damage = render_fn(width, height, buffer_age, pixels);
                render_elapsed = render_start.elapsed();
                damage
            },
        )?,
    };
    let buffer_elapsed = buffer_start.elapsed().saturating_sub(render_elapsed);
    match outcome {
        WaylandShmDrawOutcome::Presented {
            damage,
            buffer_age,
            timings,
        } => {
            if trace_frame_timing {
                let draw_elapsed = buffer_start.elapsed();
                let predraw_elapsed = buffer_start.duration_since(redraw_start);
                let input_to_present_elapsed =
                    input_latency_start.map_or(Duration::ZERO, |start| start.elapsed());
                if trace_shm_phase_timing {
                    eprintln!(
                        "[SilkSurf] shm phases: pump {:?}, ensure {:?}, acquire {:?}, seed {:?}, render {:?}, attach_damage {:?}, flush {:?}, preseed {:?}",
                        timings.pump,
                        timings.ensure,
                        timings.acquire,
                        timings.seed,
                        timings.render,
                        timings.attach_damage,
                        timings.flush,
                        timings.preseed
                    );
                }
                eprintln!(
                    "[SilkSurf] frame: {width}x{height} age {buffer_age}, damage {damage:?}, input_to_present {input_to_present_elapsed:?}, predraw {predraw_elapsed:?}, buffer {buffer_elapsed:?}, render {render_elapsed:?}, present included, draw {draw_elapsed:?}, total {:?}",
                    redraw_start.elapsed()
                );
            }
            Ok(DrawSurfaceOutcome::Presented {
                damage,
                buffer_elapsed,
                buffer_age,
                retained_tag,
            })
        }
        WaylandShmDrawOutcome::Busy => Ok(DrawSurfaceOutcome::Busy),
    }
}

fn resize_surface_kind(
    surface: &mut WinitSurfaceKind,
    current_width: &mut u32,
    current_height: &mut u32,
    width: u32,
    height: u32,
) -> bool {
    let WinitSurfaceKind::Softbuffer(surface) = surface else {
        *current_width = width;
        *current_height = height;
        return width != 0 && height != 0;
    };

    if *current_width == width && *current_height == height {
        return true;
    }

    let (Some(nonzero_width), Some(nonzero_height)) =
        (NonZeroU32::new(width), NonZeroU32::new(height))
    else {
        return false;
    };

    match surface.resize(nonzero_width, nonzero_height) {
        Ok(()) => {
            *current_width = width;
            *current_height = height;
            true
        }
        Err(e) => {
            eprintln!("[SilkSurf] softbuffer: resize error: {e}");
            false
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::{
        BLOCKING_BUSY_REDRAW_RETRY_INTERVAL, NONBLOCKING_BUSY_REDRAW_RETRY_INTERVAL,
        RedrawRequestKind, WAYLAND_REDRAW_PACE_COLD_BUFFER, WAYLAND_REDRAW_PACE_INITIAL,
        WinitCursorShape, WinitDisplayBackend, WinitInput, WinitInputProbe, WinitInputResult,
        WinitWaylandPresenter, effective_wayland_presenter, initial_redraw_pace_interval,
        input_redraw_bypass_pacing, next_redraw_pace_interval, redraw_flush_wait_deadline,
        redraw_pacing_enabled_for_backend, redraw_request_wait_deadline, resolve_display_backend,
        wayland_display_socket_path, wayland_shm_failure_uses_softbuffer_fallback,
    };
    use std::ffi::OsStr;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};
    use winit::keyboard::NamedKey;
    use winit::keyboard::{Key, ModifiersState};

    #[test]
    fn input_result_preserves_bool_redraw_compatibility() {
        assert_eq!(
            WinitInputResult::from(true),
            WinitInputResult {
                redraw: true,
                cursor: None,
            }
        );
        assert_eq!(
            WinitInputResult::cursor(WinitCursorShape::Text),
            WinitInputResult {
                redraw: false,
                cursor: Some(WinitCursorShape::Text),
            }
        );
    }

    #[test]
    fn auto_prefers_wayland_when_wayland_display_exists() {
        assert_eq!(
            resolve_display_backend(WinitDisplayBackend::Auto, true, true),
            WinitDisplayBackend::Wayland
        );
    }

    #[test]
    fn auto_uses_x11_when_only_display_exists() {
        assert_eq!(
            resolve_display_backend(WinitDisplayBackend::Auto, false, true),
            WinitDisplayBackend::X11
        );
    }

    #[test]
    fn explicit_backend_overrides_environment() {
        assert_eq!(
            resolve_display_backend(WinitDisplayBackend::X11, true, true),
            WinitDisplayBackend::X11
        );
    }

    #[test]
    fn auto_uses_x11_when_wayland_env_is_stale() {
        assert_eq!(
            resolve_display_backend(WinitDisplayBackend::Auto, false, true),
            WinitDisplayBackend::X11
        );
    }

    #[test]
    fn wayland_socket_path_uses_runtime_dir_for_display_name() {
        assert_eq!(
            wayland_display_socket_path(
                OsStr::new("wayland-0"),
                Some(OsStr::new("/run/user/1000"))
            ),
            Some(PathBuf::from("/run/user/1000/wayland-0"))
        );
    }

    #[test]
    fn wayland_socket_path_preserves_absolute_display_path() {
        assert_eq!(
            wayland_display_socket_path(OsStr::new("/tmp/wayland-test"), None),
            Some(PathBuf::from("/tmp/wayland-test"))
        );
    }

    #[test]
    fn redraw_pacing_tracks_wayland_presenter_blocking_model() {
        assert!(!redraw_pacing_enabled_for_backend(
            WinitDisplayBackend::Wayland,
            WinitWaylandPresenter::Auto
        ));
        assert!(redraw_pacing_enabled_for_backend(
            WinitDisplayBackend::Wayland,
            WinitWaylandPresenter::Softbuffer
        ));
        assert!(!redraw_pacing_enabled_for_backend(
            WinitDisplayBackend::Wayland,
            WinitWaylandPresenter::Shm
        ));
        assert!(!redraw_pacing_enabled_for_backend(
            WinitDisplayBackend::X11,
            WinitWaylandPresenter::Softbuffer
        ));
        assert!(!redraw_pacing_enabled_for_backend(
            WinitDisplayBackend::Auto,
            WinitWaylandPresenter::Softbuffer
        ));
    }

    #[test]
    fn redraw_pacing_starts_at_low_latency_wayland_interval() {
        assert_eq!(
            initial_redraw_pace_interval(
                WinitDisplayBackend::Wayland,
                WinitWaylandPresenter::Softbuffer
            ),
            WAYLAND_REDRAW_PACE_INITIAL
        );
        assert_eq!(
            initial_redraw_pace_interval(WinitDisplayBackend::X11, WinitWaylandPresenter::Auto),
            Duration::ZERO
        );
        assert_eq!(
            initial_redraw_pace_interval(WinitDisplayBackend::Wayland, WinitWaylandPresenter::Shm),
            Duration::ZERO
        );
    }

    #[test]
    fn wayland_auto_presenter_uses_shm() {
        assert_eq!(
            effective_wayland_presenter(WinitDisplayBackend::Wayland, WinitWaylandPresenter::Auto),
            WinitWaylandPresenter::Shm
        );
        assert_eq!(
            effective_wayland_presenter(WinitDisplayBackend::X11, WinitWaylandPresenter::Auto),
            WinitWaylandPresenter::Softbuffer
        );
    }

    #[test]
    fn only_auto_wayland_shm_failure_uses_softbuffer_fallback() {
        assert!(wayland_shm_failure_uses_softbuffer_fallback(
            WinitWaylandPresenter::Auto
        ));
        assert!(!wayland_shm_failure_uses_softbuffer_fallback(
            WinitWaylandPresenter::Shm
        ));
        assert!(!wayland_shm_failure_uses_softbuffer_fallback(
            WinitWaylandPresenter::Softbuffer
        ));
    }

    #[test]
    fn command_shortcuts_translate_to_clipboard_inputs() {
        assert_eq!(
            super::translate_logical_key(&Key::Character("c"), ModifiersState::CONTROL),
            Some(WinitInput::Copy)
        );
        assert_eq!(
            super::translate_logical_key(&Key::Character("v"), ModifiersState::SUPER),
            Some(WinitInput::Paste)
        );
        assert_eq!(
            super::translate_logical_key(&Key::Character("x"), ModifiersState::CONTROL),
            Some(WinitInput::Cut)
        );
    }

    #[test]
    fn command_shortcuts_do_not_emit_text_input() {
        assert_eq!(
            super::translate_logical_key(&Key::Character("c"), ModifiersState::empty()),
            Some(WinitInput::TextInput('c'))
        );
        assert_eq!(
            super::translate_logical_key(&Key::Character("c"), ModifiersState::ALT),
            None
        );
        assert_eq!(
            super::translate_logical_key(&Key::Character("c"), ModifiersState::CONTROL),
            Some(WinitInput::Copy)
        );
    }

    #[test]
    fn horizontal_arrows_translate_to_caret_inputs() {
        assert_eq!(
            super::translate_logical_key(&Key::Named(NamedKey::ArrowLeft), ModifiersState::empty()),
            Some(WinitInput::MoveCaretLeft)
        );
        assert_eq!(
            super::translate_logical_key(
                &Key::Named(NamedKey::ArrowRight),
                ModifiersState::empty()
            ),
            Some(WinitInput::MoveCaretRight)
        );
    }

    #[test]
    fn redraw_pacing_increases_only_after_buffer_waits() {
        assert_eq!(
            next_redraw_pace_interval(Duration::from_millis(1), Duration::from_millis(2), 2),
            Duration::from_millis(4)
        );
        assert_eq!(
            next_redraw_pace_interval(Duration::from_millis(12), Duration::from_millis(20), 2),
            Duration::from_millis(16)
        );
    }

    #[test]
    fn redraw_pacing_uses_warmup_interval_for_cold_buffers() {
        assert_eq!(
            next_redraw_pace_interval(Duration::from_millis(1), Duration::from_micros(50), 0),
            WAYLAND_REDRAW_PACE_COLD_BUFFER
        );
    }

    #[test]
    fn redraw_pacing_decays_after_immediate_buffer_acquire() {
        assert_eq!(
            next_redraw_pace_interval(Duration::from_millis(8), Duration::from_micros(50), 2),
            Duration::from_millis(7)
        );
        assert_eq!(
            next_redraw_pace_interval(Duration::from_millis(1), Duration::from_micros(50), 2),
            Duration::from_millis(1)
        );
    }

    #[test]
    fn paced_redraw_waits_before_deadline() {
        let now = Instant::now();
        let deadline = now + Duration::from_millis(8);

        assert_eq!(
            redraw_request_wait_deadline(Some(deadline), now, RedrawRequestKind::Paced, true),
            Some(deadline)
        );
    }

    #[test]
    fn urgent_redraw_bypasses_pacing_when_presenter_is_nonblocking() {
        let now = Instant::now();
        let deadline = now + Duration::from_millis(8);

        assert_eq!(
            redraw_request_wait_deadline(Some(deadline), now, RedrawRequestKind::Urgent, true),
            None
        );
    }

    #[test]
    fn urgent_redraw_waits_when_input_bypass_is_disabled() {
        let now = Instant::now();
        let deadline = now + Duration::from_millis(8);

        assert_eq!(
            redraw_request_wait_deadline(Some(deadline), now, RedrawRequestKind::Urgent, false),
            Some(deadline)
        );
    }

    #[test]
    fn input_redraw_bypass_applies_to_all_presenters() {
        assert!(input_redraw_bypass_pacing(
            WinitDisplayBackend::Wayland,
            WinitWaylandPresenter::Softbuffer
        ));
        assert!(input_redraw_bypass_pacing(
            WinitDisplayBackend::Wayland,
            WinitWaylandPresenter::Shm
        ));
        assert!(input_redraw_bypass_pacing(
            WinitDisplayBackend::X11,
            WinitWaylandPresenter::Auto
        ));
    }

    #[test]
    fn busy_redraw_retry_tracks_presenter_blocking_model() {
        assert_eq!(
            BLOCKING_BUSY_REDRAW_RETRY_INTERVAL,
            Duration::from_millis(16)
        );
        assert_eq!(
            NONBLOCKING_BUSY_REDRAW_RETRY_INTERVAL,
            Duration::from_millis(1)
        );
    }

    #[test]
    fn busy_redraw_deadline_coalesces_before_pacing() {
        let now = Instant::now();
        let busy_deadline = now + Duration::from_millis(16);
        let paced_deadline = now + Duration::from_millis(1);

        assert_eq!(
            redraw_flush_wait_deadline(Some(busy_deadline), Some(paced_deadline), now),
            Some(busy_deadline)
        );
    }

    #[test]
    fn expired_busy_redraw_deadline_falls_back_to_pacing() {
        let now = Instant::now();
        let busy_deadline = now
            .checked_sub(Duration::from_millis(1))
            // UNWRAP-OK: Instant::now supports subtracting one millisecond.
            .expect("instant supports a one millisecond subtraction");
        let paced_deadline = now + Duration::from_millis(4);

        assert_eq!(
            redraw_flush_wait_deadline(Some(busy_deadline), Some(paced_deadline), now),
            Some(paced_deadline)
        );
    }

    #[test]
    fn expired_redraw_deadlines_allow_request() {
        let now = Instant::now();
        let busy_deadline = now
            .checked_sub(Duration::from_millis(2))
            // UNWRAP-OK: Instant::now supports subtracting two milliseconds.
            .expect("instant supports a two millisecond subtraction");
        let paced_deadline = now
            .checked_sub(Duration::from_millis(1))
            // UNWRAP-OK: Instant::now supports subtracting one millisecond.
            .expect("instant supports a one millisecond subtraction");

        assert_eq!(
            redraw_flush_wait_deadline(Some(busy_deadline), Some(paced_deadline), now),
            None
        );
    }

    #[test]
    fn input_probe_waits_for_presented_frames() {
        let mut probe = WinitInputProbe {
            steps: super::ADDRESS_INPUT_PROBE_STEPS.to_vec(),
            wait_after_step: super::wait_after_each_step(super::ADDRESS_INPUT_PROBE_STEPS.len()),
            next_index: 0,
            waiting_for_redraw: true,
            armed: false,
            exit_after_finish: true,
            exit_frame_delay: 0,
        };

        assert_eq!(probe.next_input(), None);
        assert!(!probe.finished());
        probe.arm_next_input();
        assert_eq!(probe.next_input(), Some(WinitInput::FocusAddress));
        probe.mark_input_dispatched();
        assert!(!probe.finished());
        assert_eq!(probe.next_input(), None);
        probe.arm_next_input();
        assert_eq!(probe.next_input(), Some(WinitInput::TextInput('z')));
        probe.mark_input_dispatched();
        assert!(!probe.finished());
        probe.arm_next_input();
        assert_eq!(probe.next_input(), Some(WinitInput::TextInput('z')));
        probe.mark_input_dispatched();
        assert!(!probe.finished());
        probe.arm_next_input();
        assert!(probe.finished());
        assert_eq!(probe.next_input(), None);
    }

    #[test]
    fn smoke_probe_finishes_after_first_presented_frame() {
        let mut probe = WinitInputProbe {
            steps: Vec::new(),
            wait_after_step: Vec::new(),
            next_index: 0,
            waiting_for_redraw: true,
            armed: false,
            exit_after_finish: true,
            exit_frame_delay: 0,
        };

        assert_eq!(probe.next_input(), None);
        assert!(!probe.finished());
        probe.arm_next_input();
        assert!(probe.finished());
        assert_eq!(probe.next_input(), None);
    }

    #[test]
    fn input_probe_retries_until_input_changes_state() {
        let mut probe = WinitInputProbe {
            steps: super::CHROME_INPUT_PROBE_STEPS.to_vec(),
            wait_after_step: super::wait_after_each_step(super::CHROME_INPUT_PROBE_STEPS.len()),
            next_index: 0,
            waiting_for_redraw: true,
            armed: false,
            exit_after_finish: true,
            exit_frame_delay: 0,
        };

        probe.arm_next_input();
        assert_eq!(
            probe.next_input(),
            Some(WinitInput::PrimaryClick { x: 55.0, y: 22.0 })
        );
        assert_eq!(
            probe.next_input(),
            Some(WinitInput::PrimaryClick { x: 55.0, y: 22.0 })
        );
        probe.mark_input_dispatched();
        assert_eq!(probe.next_input(), None);
    }

    #[test]
    fn page_input_probe_focuses_then_types() {
        assert_eq!(
            super::PAGE_INPUT_PROBE_STEPS,
            &[WinitInput::FocusNextPageInput, WinitInput::TextInput('!')]
        );
    }

    #[test]
    fn runtime_text_probe_waits_one_extra_frame() {
        let mut probe = WinitInputProbe {
            steps: Vec::new(),
            wait_after_step: Vec::new(),
            next_index: 0,
            waiting_for_redraw: true,
            armed: false,
            exit_after_finish: true,
            exit_frame_delay: 1,
        };

        probe.arm_next_input();
        assert!(probe.finished());
        assert!(!probe.exit_delay_elapsed());
        assert!(probe.exit_delay_elapsed());
    }

    #[test]
    fn hover_probe_moves_over_fixture_link_then_away() {
        assert_eq!(
            super::HOVER_INPUT_PROBE_STEPS,
            &[
                WinitInput::CursorMoved { x: 48.0, y: 220.0 },
                WinitInput::CursorMoved { x: 420.0, y: 420.0 },
            ]
        );
    }

    #[test]
    fn address_caret_probe_coalesces_edit_burst() {
        assert_eq!(
            super::address_caret_probe_wait_steps(),
            vec![false, false, false, false, true]
        );
    }

    #[test]
    fn form_submit_probe_focuses_types_then_submits() {
        assert_eq!(
            super::FORM_SUBMIT_PROBE_STEPS,
            &[
                WinitInput::FocusNextPageInput,
                WinitInput::TextInput('!'),
                WinitInput::SubmitAddress
            ]
        );
    }

    #[test]
    fn reload_probe_submits_reload_input() {
        assert_eq!(super::RELOAD_INPUT_PROBE_STEPS, &[WinitInput::Reload]);
    }

    #[test]
    fn stop_probe_sequence_types_url_then_clicks_stop() {
        let target = "http://127.0.0.1:9/slow";
        let mut steps = Vec::new();
        let mut wait_after_step = Vec::new();
        steps.push(WinitInput::FocusAddress);
        wait_after_step.push(true);
        for ch in target.chars() {
            steps.push(WinitInput::TextInput(ch));
            wait_after_step.push(false);
        }
        steps.push(WinitInput::SubmitAddress);
        wait_after_step.push(true);
        steps.push(WinitInput::PrimaryClick { x: 95.0, y: 22.0 });
        wait_after_step.push(true);

        assert_eq!(steps.first(), Some(&WinitInput::FocusAddress));
        assert_eq!(wait_after_step.first(), Some(&true));
        assert!(wait_after_step[1..=target.len()].iter().all(|wait| !wait));
        assert_eq!(
            steps.get(target.len() + 1),
            Some(&WinitInput::SubmitAddress)
        );
        assert_eq!(wait_after_step.get(target.len() + 1), Some(&true));
        assert_eq!(
            steps.last(),
            Some(&WinitInput::PrimaryClick { x: 95.0, y: 22.0 })
        );
    }

    #[test]
    fn stop_probe_builder_coalesces_typed_url() {
        let (steps, wait_after_step) = super::build_stop_probe_steps_for_target("http://x/slow/");
        assert_eq!(steps.first(), Some(&WinitInput::FocusAddress));
        assert_eq!(wait_after_step.first(), Some(&true));
        assert_eq!(
            steps.last(),
            Some(&WinitInput::PrimaryClick { x: 95.0, y: 22.0 })
        );
        assert_eq!(wait_after_step.last(), Some(&true));
        assert!(
            wait_after_step[1..wait_after_step.len() - 2]
                .iter()
                .all(|wait| !wait)
        );
        assert_eq!(steps.get(steps.len() - 2), Some(&WinitInput::SubmitAddress));
    }
}
