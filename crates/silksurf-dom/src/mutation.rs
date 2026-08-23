/*
 * The mutation record queue DOM 4.3's "queue a mutation record" fills.
 *
 * Records live on the tree rather than on the JS bridge because a mutation
 * reaches the tree through more paths than the bridge owns: the innerHTML
 * setter splices through `Dom::import_subtree`, and the fragment parser builds
 * through the ordinary constructors. One queue behind the mutators sees every
 * one of them.
 *
 * Recording opens on an exact request, so a document with no observer pays one
 * bool test per mutation and allocates nothing.
 */

use std::collections::HashSet;

use crate::NodeId;

/// What changed about a node, in the three shapes a `MutationRecord` takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MutationKind {
    /// Children were added, removed, or both, in one tree operation.
    ChildList {
        added: Vec<NodeId>,
        removed: Vec<NodeId>,
        previous: Option<NodeId>,
        next: Option<NodeId>,
    },
    /// An attribute was set or removed. `old` carries the value the attribute
    /// held before, which `attributeOldValue` reports.
    Attributes { name: String, old: Option<String> },
    /// A text node's data was replaced. `old` carries the previous data, which
    /// `characterDataOldValue` reports.
    CharacterData { old: String },
}

/// One queued mutation. `target` is the node the record is reported against:
/// the parent for a child list change, the element for an attribute change,
/// and the text node itself for character data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MutationRecord {
    pub target: NodeId,
    pub kind: MutationKind,
}

/// The queue and the two filters that decide what enters it.
#[derive(Debug, Default)]
pub(crate) struct MutationLog {
    recording: bool,
    records: Vec<MutationRecord>,
    /// Nodes added to the tree since the last take. A mutation whose target
    /// sits under one of them describes a subtree the observer never saw
    /// separately, so it reports through the record that added the subtree.
    /// This is what makes `import_subtree` read as one splice rather than as
    /// one record per imported descendant. A set rather than a list, because a
    /// reconcile that splices hundreds of nodes between takes would otherwise
    /// scan every earlier addition per mutation.
    added: HashSet<NodeId>,
}

impl MutationLog {
    pub(crate) fn set_recording(&mut self, on: bool) {
        self.recording = on;
        if !on {
            self.records.clear();
            self.added.clear();
        }
    }

    pub(crate) fn recording(&self) -> bool {
        self.recording
    }

    pub(crate) fn pending(&self) -> usize {
        self.records.len()
    }

    pub(crate) fn take(&mut self) -> Vec<MutationRecord> {
        self.added.clear();
        std::mem::take(&mut self.records)
    }

    pub(crate) fn was_added(&self, id: NodeId) -> bool {
        self.added.contains(&id)
    }

    pub(crate) fn push(&mut self, record: MutationRecord) {
        self.absorb(&record.kind);
        self.records.push(record);
    }

    /// Take a kind's added nodes into the added set without queuing a record,
    /// which is how suppression propagates into a subtree.
    pub(crate) fn absorb(&mut self, kind: &MutationKind) {
        if let MutationKind::ChildList { added, .. } = kind {
            self.added.extend(added.iter().copied());
        }
    }
}
