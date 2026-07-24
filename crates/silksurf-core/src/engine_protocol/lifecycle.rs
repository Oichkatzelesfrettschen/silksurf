//! Engine, view, and frame lifecycle state machines.
//!
//! Transitions are validated against explicit tables. An illegal edge returns
//! `IllegalTransition` and leaves the caller's state unchanged, so a peer that
//! reports states out of order cannot drive the tracker into a nonsense state.

use thiserror::Error;

/// An attempted state change that no transition table permits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("illegal transition {from} -> {to}")]
pub struct IllegalTransition {
    /// Source state name.
    pub from: &'static str,
    /// Rejected target state name.
    pub to: &'static str,
}

/// Lifecycle of one engine process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineState {
    /// Spawned, not yet ready.
    Starting,
    /// Serving commands.
    Ready,
    /// Finishing outstanding work before exit.
    Draining,
    /// Stopped cleanly.
    Exited,
    /// Stopped abnormally.
    Failed,
}

impl EngineState {
    const TRANSITIONS: &'static [(Self, Self)] = &[
        (Self::Starting, Self::Ready),
        (Self::Starting, Self::Failed),
        (Self::Ready, Self::Draining),
        (Self::Ready, Self::Failed),
        (Self::Draining, Self::Exited),
        (Self::Draining, Self::Failed),
    ];

    /// The state's name, for diagnostics.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Starting => "Starting",
            Self::Ready => "Ready",
            Self::Draining => "Draining",
            Self::Exited => "Exited",
            Self::Failed => "Failed",
        }
    }

    /// Whether the transition to `to` is permitted.
    pub fn can_transition(self, to: Self) -> bool {
        Self::TRANSITIONS
            .iter()
            .any(|&(from, dest)| from == self && dest == to)
    }

    /// Applies the transition or reports it illegal.
    pub fn transition(self, to: Self) -> Result<Self, IllegalTransition> {
        if self.can_transition(to) {
            Ok(to)
        } else {
            Err(IllegalTransition {
                from: self.name(),
                to: to.name(),
            })
        }
    }
}

/// Lifecycle of one view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewState {
    /// Requested, not yet loading.
    Creating,
    /// Loading a document.
    Loading,
    /// Document interactive.
    Interactive,
    /// Closing.
    Closing,
    /// Closed.
    Closed,
    /// Failed.
    Failed,
}

impl ViewState {
    const TRANSITIONS: &'static [(Self, Self)] = &[
        (Self::Creating, Self::Loading),
        (Self::Creating, Self::Failed),
        (Self::Loading, Self::Interactive),
        (Self::Loading, Self::Failed),
        (Self::Interactive, Self::Loading),
        (Self::Interactive, Self::Closing),
        (Self::Interactive, Self::Failed),
        (Self::Closing, Self::Closed),
        (Self::Failed, Self::Closing),
    ];

    /// The state's name, for diagnostics.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Creating => "Creating",
            Self::Loading => "Loading",
            Self::Interactive => "Interactive",
            Self::Closing => "Closing",
            Self::Closed => "Closed",
            Self::Failed => "Failed",
        }
    }

    /// Whether the transition to `to` is permitted.
    pub fn can_transition(self, to: Self) -> bool {
        Self::TRANSITIONS
            .iter()
            .any(|&(from, dest)| from == self && dest == to)
    }

    /// Applies the transition or reports it illegal.
    pub fn transition(self, to: Self) -> Result<Self, IllegalTransition> {
        if self.can_transition(to) {
            Ok(to)
        } else {
            Err(IllegalTransition {
                from: self.name(),
                to: to.name(),
            })
        }
    }
}

/// Lifecycle of one produced frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameState {
    /// Rendered, not yet handed over.
    Produced,
    /// Handed to the shell.
    Transferred,
    /// On screen.
    Presented,
    /// Superseded before presentation.
    Discarded,
    /// Ownership returned to the engine.
    Released,
}

impl FrameState {
    const TRANSITIONS: &'static [(Self, Self)] = &[
        (Self::Produced, Self::Transferred),
        (Self::Transferred, Self::Presented),
        (Self::Transferred, Self::Discarded),
        (Self::Presented, Self::Released),
        (Self::Discarded, Self::Released),
    ];

    /// The state's name, for diagnostics.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Produced => "Produced",
            Self::Transferred => "Transferred",
            Self::Presented => "Presented",
            Self::Discarded => "Discarded",
            Self::Released => "Released",
        }
    }

    /// Whether the transition to `to` is permitted.
    pub fn can_transition(self, to: Self) -> bool {
        Self::TRANSITIONS
            .iter()
            .any(|&(from, dest)| from == self && dest == to)
    }

    /// Applies the transition or reports it illegal.
    pub fn transition(self, to: Self) -> Result<Self, IllegalTransition> {
        if self.can_transition(to) {
            Ok(to)
        } else {
            Err(IllegalTransition {
                from: self.name(),
                to: to.name(),
            })
        }
    }
}
