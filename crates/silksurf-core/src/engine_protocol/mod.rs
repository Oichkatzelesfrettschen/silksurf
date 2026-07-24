//! Engine protocol v1: the process-neutral, view-oriented boundary between the
//! SilkSurf shell and a page engine.
//!
//! The boundary carries browser-view operations only. It never exposes `Dom`,
//! `NodeId`, Boa values, CSS structures, Taffy nodes, or display-list entries,
//! so a non-native backend (WPE, Wry, Servo, CEF) can satisfy the same
//! contract. The design is specified in `docs/design/ENGINE-PROTOCOL-V1.md`
//! and decided in `ARCHITECTURE-DECISIONS.md` AD-027.
//!
//! The protocol has two planes. The control plane -- `Command`, `Event`, and
//! `InputEvent` -- is fully serialized, decoded, and validated here; a
//! malformed control message decodes to a typed `ProtocolError` and never
//! panics the receiver. The frame plane -- `FrameHandle` -- is an abstract
//! descriptor; its concrete transport (sealed shared memory first, DMA-BUF
//! later) is bound at the native-runtime extraction spike (issue #53).
//!
//! This module homes in `silksurf-core` because both the shell and the native
//! engine already depend on it. It is a split candidate: once #53 measures
//! ownership it extracts to a dedicated `silksurf-engine-protocol` crate.

pub mod codec;
pub mod ids;
pub mod lifecycle;
pub mod message;
pub mod version;
pub mod wire;

pub use codec::{MAX_DAMAGE_RECTS, MAX_MESSAGE_BYTES, MAX_STRING_BYTES, ProtocolError};
pub use ids::{EngineInstanceId, FrameGeneration, ProfileId, RequestId, ViewId};
pub use lifecycle::{EngineState, FrameState, IllegalTransition, ViewState};
pub use message::{
    Command, CrashReason, CursorKind, DamageRect, EngineMetrics, Event, FrameHandle,
    FrameTransport, ImeKind, InputEvent, KeyKind, LoadState, Modifiers, MouseButton,
    NavigationRequest, PermissionDecision, PermissionKind, PointerKind, Viewport,
};
pub use version::{
    Capabilities, Endpoint, Negotiated, ProtocolVersion, VersionError, VersionRange, negotiate,
    negotiate_version,
};
pub use wire::{Message, WIRE_VERSION};
