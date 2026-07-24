//! Opaque identifiers exchanged across the engine boundary.
//!
//! Each id is a `u64` newtype meaningful only to the allocator that mints it;
//! the peer treats it as a routing token. `FrameGeneration` is monotonic per
//! view and gates frame release so a stale engine cannot present over a live
//! frame.

/// One running engine process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EngineInstanceId(u64);

/// One browsing view (a future tab) inside an engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ViewId(u64);

/// One persistent profile: cookie jar, storage, and history root.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProfileId(u64);

/// One outstanding request/response pair (permission, download, file chooser,
/// or new view).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RequestId(u64);

/// A monotonic frame counter, unique per view. A newer generation supersedes
/// an older one; the shell releases a frame by its generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FrameGeneration(u64);

macro_rules! raw_u64_id {
    ($name:ident) => {
        impl $name {
            /// Wraps a raw id value.
            pub const fn new(value: u64) -> Self {
                Self(value)
            }

            /// The raw id value.
            pub const fn get(self) -> u64 {
                self.0
            }
        }
    };
}

raw_u64_id!(EngineInstanceId);
raw_u64_id!(ViewId);
raw_u64_id!(ProfileId);
raw_u64_id!(RequestId);
raw_u64_id!(FrameGeneration);

impl FrameGeneration {
    /// The first generation of a view.
    pub const FIRST: Self = Self(1);

    /// The next generation. Saturates at `u64::MAX` rather than wrapping, so
    /// ordering stays monotonic for the life of a view.
    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}
