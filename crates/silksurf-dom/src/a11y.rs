//! Accessibility tree (skeleton).
//!
//! WHY: a real browser must expose an accessibility tree so assistive
//! technologies (screen readers, switch devices, voice control) can
//! navigate the rendered document. The Linux convention is AT-SPI;
//! Windows is UIA; macOS is the AX API. silksurf v0.1 ships Linux-first
//! (see ADR-010), so AT-SPI is the long-term target.
//!
//! WHAT: this module is the skeleton. The full a11y-tree builder will
//! live here and consume the styled DOM + layout tree to produce
//! `AccessibilityNode`s with role, name, state, and relationship
//! information per W3C WAI-ARIA 1.2 + ARIA-in-HTML.
//!
//! HOW (planned):
//!
//!   1. `build_a11y_tree(dom, styles, layout) -> AccessibilityTree`
//!      walks the DOM, derives a role for each element from the tag
//!      name and any `role=` attribute, computes accessible name via
//!      the WAI-ARIA name-computation algorithm, captures focus and
//!      state.
//!   2. `silksurf-gui` exposes the tree via AT-SPI on Linux.
//!   3. Conformance: WAI-ARIA 1.2 + ARIA-in-HTML test suite (TBD).
//!
//! Tracked in the SNAZZY-WAFFLE roadmap P8.S5.

use silksurf_dom_internal::NodeId;

/// Root of the accessibility tree -- placeholder.
#[derive(Debug, Default)]
pub struct AccessibilityTree {
    /// Per-DOM-node accessibility metadata. Empty until P8.S5 lands.
    pub nodes: Vec<AccessibilityNode>,
}

/// Per-element accessibility metadata -- placeholder.
#[derive(Debug)]
pub struct AccessibilityNode {
    pub dom_node: NodeId,
    pub role: AccessibilityRole,
}

/// Subset of WAI-ARIA 1.2 roles. Will expand to the full ~80-role set
/// when P8.S5 lands.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum AccessibilityRole {
    Generic,
    Button,
    Link,
    Heading,
    Image,
    Textbox,
    List,
    ListItem,
}

// Re-export NodeId locally so this stub does not need a fresh import.
mod silksurf_dom_internal {
    pub use crate::NodeId;
}
