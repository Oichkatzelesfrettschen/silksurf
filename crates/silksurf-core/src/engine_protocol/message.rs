//! Control-plane and frame-plane data types.
//!
//! These are browser-view operations only. No `Dom`, `NodeId`, Boa value, CSS
//! structure, Taffy node, or display-list entry crosses the boundary. Small
//! enums carry a `u8` discriminant mapping used by the wire codec; the large
//! command, event, and input enums are serialized in `wire`.

use super::ids::{FrameGeneration, ProfileId, RequestId, ViewId};
use super::version::Capabilities;

/// The rendered size and scale of a view, in physical device pixels. Scale is
/// fixed-point per-mille (1000 = 1.0) so it needs no float on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Viewport {
    /// Width in physical pixels.
    pub width: u32,
    /// Height in physical pixels.
    pub height: u32,
    /// Device scale in per-mille units (1000 = 1.0).
    pub scale_permille: u16,
}

/// A changed rectangle of a frame, in physical pixels from the view origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DamageRect {
    /// Left edge.
    pub x: u32,
    /// Top edge.
    pub y: u32,
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
}

/// Where a view is in its load. Ordering follows navigation progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadState {
    /// No navigation in flight.
    Idle,
    /// A navigation has started.
    Started,
    /// The response committed; the new document is live.
    Committed,
    /// The document is interactive.
    Interactive,
    /// Load finished.
    Complete,
    /// Load failed.
    Failed,
}

/// The cursor a view requests over its surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorKind {
    /// Default arrow.
    Default,
    /// Link/clickable pointer.
    Pointer,
    /// Text/I-beam.
    Text,
    /// Busy.
    Wait,
    /// Background progress.
    Progress,
    /// Precise crosshair.
    Crosshair,
    /// Action not allowed.
    NotAllowed,
    /// Grabbable.
    Grab,
    /// Actively grabbing.
    Grabbing,
}

/// The kind of a pointer input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerKind {
    /// Button pressed.
    Down,
    /// Button released.
    Up,
    /// Pointer moved.
    Move,
    /// Pointer entered the surface.
    Enter,
    /// Pointer left the surface.
    Leave,
    /// Wheel scrolled.
    Wheel,
}

/// A pointer button. `None` accompanies moves and wheels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    /// No button.
    None,
    /// Primary.
    Left,
    /// Tertiary.
    Middle,
    /// Secondary.
    Right,
}

/// The kind of a key input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyKind {
    /// Key pressed.
    Down,
    /// Key released.
    Up,
}

/// The phase of an input-method composition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImeKind {
    /// Provisional composition text.
    Preedit,
    /// Composition committed.
    Commit,
    /// Composition cancelled.
    Cancel,
}

/// Keyboard modifier bitset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Modifiers(u8);

impl Modifiers {
    /// No modifiers.
    pub const NONE: Self = Self(0);
    /// Shift held.
    pub const SHIFT: Self = Self(1 << 0);
    /// Control held.
    pub const CTRL: Self = Self(1 << 1);
    /// Alt held.
    pub const ALT: Self = Self(1 << 2);
    /// Meta/Super/Command held.
    pub const META: Self = Self(1 << 3);

    /// The raw bits.
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// From raw bits.
    pub const fn from_bits(bits: u8) -> Self {
        Self(bits)
    }

    /// Whether every bit in `other` is set.
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// This set with `other` added.
    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// A single input delivered to a view. Coordinates are physical device pixels
/// relative to the view origin and may be negative outside the surface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputEvent {
    /// Pointer motion, button, or wheel.
    Pointer {
        /// Pointer phase.
        kind: PointerKind,
        /// X in physical pixels.
        x: i32,
        /// Y in physical pixels.
        y: i32,
        /// Button, or `None` for move/wheel.
        button: MouseButton,
        /// Horizontal wheel delta.
        wheel_x: i32,
        /// Vertical wheel delta.
        wheel_y: i32,
    },
    /// Key press or release.
    Key {
        /// Key phase.
        kind: KeyKind,
        /// Platform-neutral key code.
        key_code: u32,
        /// Held modifiers.
        modifiers: Modifiers,
    },
    /// Committed text (post-composition).
    Text {
        /// The text.
        text: String,
    },
    /// Input-method composition.
    Ime {
        /// Composition phase.
        kind: ImeKind,
        /// Composition text.
        text: String,
        /// Selection start in the composition, in bytes.
        cursor_begin: u32,
        /// Selection end in the composition, in bytes.
        cursor_end: u32,
    },
    /// Keyboard focus gained or lost.
    Focus {
        /// Whether focus was gained.
        gained: bool,
    },
}

/// The target of a navigation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavigationRequest {
    /// Destination URL.
    pub url: String,
}

/// A permission a view asks the user to grant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionKind {
    /// Geolocation.
    Geolocation,
    /// Desktop notifications.
    Notifications,
    /// Camera.
    Camera,
    /// Microphone.
    Microphone,
    /// Clipboard read.
    ClipboardRead,
    /// Clipboard write.
    ClipboardWrite,
    /// MIDI system-exclusive.
    MidiSysex,
    /// Persistent storage.
    Storage,
}

/// The user's answer to a permission request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionDecision {
    /// Granted.
    Grant,
    /// Denied.
    Deny,
}

/// Why an engine or view stopped abnormally.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrashReason {
    /// Unhandled panic.
    Panic,
    /// Out of memory.
    OutOfMemory,
    /// Killed by signal or supervisor.
    Killed,
    /// Rejected a peer protocol violation.
    ProtocolViolation,
    /// Watchdog timeout.
    Timeout,
}

/// Coarse per-view runtime metrics for the shell diagnostics surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineMetrics {
    /// Last input-to-frame-submit in microseconds.
    pub input_to_submit_us: u64,
    /// Engine resident set size in bytes.
    pub rss_bytes: u64,
    /// Frame copies for the last presented frame.
    pub frame_copies: u32,
}

/// How a rendered frame's pixels are transported. The concrete representation
/// of `token` (shared-memory name or DMA-BUF registry key) is bound at the
/// native-runtime extraction spike (issue #53); here it is an opaque handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameTransport {
    /// Sealed shared memory. `token` names the mapping; `len` is its byte size.
    SharedMemory {
        /// Opaque mapping handle.
        token: u64,
        /// Mapping length in bytes.
        len: u64,
    },
    /// Platform frame handle (DMA-BUF). `token` names the import; `len` is its
    /// byte size.
    Platform {
        /// Opaque import handle.
        token: u64,
        /// Import length in bytes.
        len: u64,
    },
}

/// A produced frame offered to the shell. The generation gates release so a
/// stale engine cannot present over a live frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameHandle {
    /// Owning view.
    pub view: ViewId,
    /// Monotonic frame generation.
    pub generation: FrameGeneration,
    /// Pixel transport.
    pub transport: FrameTransport,
}

/// A shell-to-engine command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Create a view in a profile at a viewport.
    CreateView {
        /// New view id.
        view: ViewId,
        /// Profile to attach.
        profile: ProfileId,
        /// Initial viewport.
        viewport: Viewport,
    },
    /// Close a view.
    CloseView {
        /// View to close.
        view: ViewId,
    },
    /// Navigate a view.
    Navigate {
        /// Target view.
        view: ViewId,
        /// Navigation target.
        request: NavigationRequest,
    },
    /// Reload a view.
    Reload {
        /// Target view.
        view: ViewId,
    },
    /// Stop a view's load.
    Stop {
        /// Target view.
        view: ViewId,
    },
    /// Resize a view.
    Resize {
        /// Target view.
        view: ViewId,
        /// New viewport.
        viewport: Viewport,
    },
    /// Set a view's visibility.
    SetVisible {
        /// Target view.
        view: ViewId,
        /// Visible when true.
        visible: bool,
    },
    /// Deliver input to a view.
    Input {
        /// Target view.
        view: ViewId,
        /// The input.
        event: InputEvent,
    },
    /// Answer a permission request.
    PermissionDecision {
        /// Request being answered.
        request: RequestId,
        /// The decision.
        decision: PermissionDecision,
    },
    /// Return a presented frame to the engine.
    ReleaseFrame {
        /// Frame's view.
        view: ViewId,
        /// Frame's generation.
        generation: FrameGeneration,
    },
    /// Begin a clean shutdown.
    Shutdown,
}

/// An engine-to-shell event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// A view was created.
    ViewCreated {
        /// The view.
        view: ViewId,
    },
    /// A view was closed.
    ViewClosed {
        /// The view.
        view: ViewId,
    },
    /// A view's load state changed.
    LoadStateChanged {
        /// The view.
        view: ViewId,
        /// New load state.
        state: LoadState,
    },
    /// A view's URL changed.
    UrlChanged {
        /// The view.
        view: ViewId,
        /// New URL.
        url: String,
    },
    /// A view's title changed.
    TitleChanged {
        /// The view.
        view: ViewId,
        /// New title.
        title: String,
    },
    /// A view's cursor changed.
    CursorChanged {
        /// The view.
        view: ViewId,
        /// New cursor.
        cursor: CursorKind,
    },
    /// A view's status text changed.
    StatusChanged {
        /// The view.
        view: ViewId,
        /// New status text.
        status: String,
    },
    /// A view's load progress changed.
    ProgressChanged {
        /// The view.
        view: ViewId,
        /// Progress in per-mille (0..=1000).
        permille: u16,
    },
    /// A view requests a permission.
    PermissionRequested {
        /// Request id to answer.
        request: RequestId,
        /// The view.
        view: ViewId,
        /// Permission asked.
        kind: PermissionKind,
        /// Requesting origin.
        origin: String,
    },
    /// A view starts a download.
    DownloadRequested {
        /// Request id.
        request: RequestId,
        /// The view.
        view: ViewId,
        /// Download URL.
        url: String,
        /// Suggested file name.
        suggested_name: String,
    },
    /// A view opens a file chooser.
    FileChooserRequested {
        /// Request id.
        request: RequestId,
        /// The view.
        view: ViewId,
        /// Whether multiple files may be chosen.
        multiple: bool,
    },
    /// A view requests a new view (popup or target).
    NewViewRequested {
        /// Request id.
        request: RequestId,
        /// Originating view.
        source_view: ViewId,
        /// Target URL.
        url: String,
    },
    /// A frame is ready with its damage.
    FrameReady {
        /// The frame.
        frame: FrameHandle,
        /// Changed rectangles.
        damage: Vec<DamageRect>,
    },
    /// A view crashed.
    Crashed {
        /// The view.
        view: ViewId,
        /// Why.
        reason: CrashReason,
    },
    /// A view is unresponsive.
    Hang {
        /// The view.
        view: ViewId,
        /// How long it has been unresponsive, in milliseconds.
        elapsed_ms: u64,
    },
    /// A command needed a capability the engine did not advertise.
    CapabilityMismatch {
        /// The view.
        view: ViewId,
        /// The missing capability bits.
        needed: Capabilities,
    },
    /// Per-view metrics sample.
    Metrics {
        /// The view.
        view: ViewId,
        /// The sample.
        sample: EngineMetrics,
    },
}

macro_rules! byte_enum {
    ($name:ident { $($variant:ident = $value:literal),+ $(,)? }) => {
        impl $name {
            /// The wire discriminant.
            pub const fn to_u8(self) -> u8 {
                match self {
                    $(Self::$variant => $value,)+
                }
            }

            /// Parses a wire discriminant, or `None` if out of range.
            pub const fn from_u8(value: u8) -> Option<Self> {
                match value {
                    $($value => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

byte_enum!(LoadState {
    Idle = 0,
    Started = 1,
    Committed = 2,
    Interactive = 3,
    Complete = 4,
    Failed = 5,
});

byte_enum!(CursorKind {
    Default = 0,
    Pointer = 1,
    Text = 2,
    Wait = 3,
    Progress = 4,
    Crosshair = 5,
    NotAllowed = 6,
    Grab = 7,
    Grabbing = 8,
});

byte_enum!(PointerKind {
    Down = 0,
    Up = 1,
    Move = 2,
    Enter = 3,
    Leave = 4,
    Wheel = 5,
});

byte_enum!(MouseButton {
    None = 0,
    Left = 1,
    Middle = 2,
    Right = 3,
});

byte_enum!(KeyKind {
    Down = 0,
    Up = 1,
});

byte_enum!(ImeKind {
    Preedit = 0,
    Commit = 1,
    Cancel = 2,
});

byte_enum!(PermissionKind {
    Geolocation = 0,
    Notifications = 1,
    Camera = 2,
    Microphone = 3,
    ClipboardRead = 4,
    ClipboardWrite = 5,
    MidiSysex = 6,
    Storage = 7,
});

byte_enum!(PermissionDecision {
    Grant = 0,
    Deny = 1,
});

byte_enum!(CrashReason {
    Panic = 0,
    OutOfMemory = 1,
    Killed = 2,
    ProtocolViolation = 3,
    Timeout = 4,
});
