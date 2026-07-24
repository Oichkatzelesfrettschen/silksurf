//! Protocol version and capability negotiation.
//!
//! A major version bump is a breaking change: different majors never
//! interoperate. Within a shared major, two peers agree on the highest common
//! minor. Capabilities negotiate as the intersection of what each side
//! advertises, so a command that needs an unadvertised capability is refused
//! with an event rather than assumed present.

use thiserror::Error;

/// A protocol version. Ordering compares major first, then minor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProtocolVersion {
    /// Breaking-change axis. Different majors are incompatible.
    pub major: u16,
    /// Backward-compatible feature axis within a major.
    pub minor: u16,
}

impl ProtocolVersion {
    /// The version this build speaks.
    pub const CURRENT: Self = Self { major: 1, minor: 0 };

    /// Constructs a version.
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }
}

/// A contiguous range of versions a peer speaks, within one major.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VersionRange {
    /// Lowest supported version.
    pub min: ProtocolVersion,
    /// Highest supported version.
    pub max: ProtocolVersion,
}

impl VersionRange {
    /// A range spanning `min..=max`.
    pub const fn new(min: ProtocolVersion, max: ProtocolVersion) -> Self {
        Self { min, max }
    }

    /// A range covering only the current version.
    pub const fn current() -> Self {
        Self {
            min: ProtocolVersion::CURRENT,
            max: ProtocolVersion::CURRENT,
        }
    }
}

/// Why version negotiation failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum VersionError {
    /// The peers speak different, incompatible majors.
    #[error("major mismatch: {local} vs {remote}")]
    MajorMismatch { local: u16, remote: u16 },
    /// Same major, but the minor ranges do not overlap.
    #[error("no common version in shared major {major}")]
    NoCommonVersion { major: u16 },
}

/// One side of a negotiation: the versions it speaks and what it can do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Endpoint {
    /// Supported version range.
    pub versions: VersionRange,
    /// Advertised capabilities.
    pub capabilities: Capabilities,
}

/// The agreed protocol version and the capabilities both sides share.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Negotiated {
    /// The version both peers will speak.
    pub version: ProtocolVersion,
    /// The intersection of advertised capabilities.
    pub capabilities: Capabilities,
}

/// Agrees on the highest common version within a shared major.
///
/// The ranges are single-major; the shared major is taken from each `max`.
pub fn negotiate_version(
    local: VersionRange,
    remote: VersionRange,
) -> Result<ProtocolVersion, VersionError> {
    let major = local.max.major;
    if major != remote.max.major {
        return Err(VersionError::MajorMismatch {
            local: major,
            remote: remote.max.major,
        });
    }
    let low = local.min.max(remote.min);
    let high = local.max.min(remote.max);
    if low > high {
        return Err(VersionError::NoCommonVersion { major });
    }
    Ok(high)
}

/// Agrees on a version and intersects capabilities.
pub fn negotiate(local: Endpoint, remote: Endpoint) -> Result<Negotiated, VersionError> {
    let version = negotiate_version(local.versions, remote.versions)?;
    Ok(Negotiated {
        version,
        capabilities: local.capabilities.intersection(remote.capabilities),
    })
}

/// A bitset of optional behaviors a backend advertises. The negotiated set is
/// the intersection; a command needing an absent capability is answered with
/// `Event::CapabilityMismatch`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities(u64);

impl Capabilities {
    /// No capabilities.
    pub const EMPTY: Self = Self(0);
    /// Frames delivered through a platform handle (DMA-BUF) rather than shared
    /// memory.
    pub const DMABUF_FRAMES: Self = Self(1 << 0);
    /// File download requests.
    pub const DOWNLOADS: Self = Self(1 << 1);
    /// File chooser requests.
    pub const FILE_CHOOSER: Self = Self(1 << 2);
    /// Input-method composition events.
    pub const IME: Self = Self(1 << 3);
    /// Permission prompts.
    pub const PERMISSIONS: Self = Self(1 << 4);
    /// New-view (popup/target) requests.
    pub const NEW_VIEW: Self = Self(1 << 5);
    /// Streaming Fetch bodies.
    pub const STREAMING_FETCH: Self = Self(1 << 6);
    /// WebSocket transport.
    pub const WEBSOCKET: Self = Self(1 << 7);
    /// Accessibility tree export.
    pub const ACCESSIBILITY: Self = Self(1 << 8);
    /// Remote developer-tools inspection.
    pub const DEVTOOLS: Self = Self(1 << 9);

    /// The raw bitset.
    pub const fn bits(self) -> u64 {
        self.0
    }

    /// Constructs from a raw bitset.
    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    /// Whether every bit in `other` is set.
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// The union of two sets.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// The intersection of two sets.
    #[must_use]
    pub const fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    /// This set with `other` added.
    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        self.union(other)
    }
}
