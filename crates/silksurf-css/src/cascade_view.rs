/*
 * cascade_view.rs -- SoA materialized view of DOM data for the cascade hot path.
 *
 * WHY: Node is 168 bytes (2.6 cache lines). During cascade, only tag + id +
 * class data is accessed -- parent/children (48+ bytes) are dead weight that
 * pollutes the cache. Attribute is 184 bytes with heap indirection via Vec.
 *
 * CascadeView extracts cascade-relevant fields into a compact SoA layout:
 *   entries: Vec<CascadeEntry>  -- 36 bytes per node (fits 1 cache line)
 *   idents: Vec<SelectorIdent>  -- flat array of all id+class SelectorIdents
 *
 * Per-node cascade access: one array index into entries (1 cache line), then
 * a slice into the contiguous idents array for class lookups. No NodeKind
 * pattern match, no attribute Vec iteration, no SelectorIdent construction.
 *
 * LIFECYCLE: Built at the same phase boundaries as Dom::resolve_table:
 *   1. After TreeBuilder::into_dom() -- initial materialization
 *   2. After Dom::end_mutation_batch() -- incremental update
 *
 * Because CascadeView stores SelectorIdent (from silksurf-css), it cannot
 * live on Dom (silksurf-dom) without creating a dependency cycle. It lives
 * in silksurf-css and is built by the consumer (FusedWorkspace / cascade).
 *
 * See: style.rs cascade_for_node() for the consumer
 * See: dom/lib.rs Node (168 bytes) for what this replaces
 */

use crate::selector::SelectorIdent;
use silksurf_dom::{AttributeName, Dom, NodeId, NodeKind, TagName};

/*
 * CascadeEntry -- compact per-node cascade data.
 *
 * Layout: 24 (TagName) + 4 (id_index) + 4 (class_start) + 2 (class_count)
 *       + 2 (padding) = 36 bytes. Fits in a single 64-byte cache line.
 *
 * TagName is 24 bytes (enum with Custom(SmolStr) variant). Known tags like
 * Div, Span, P use zero-size discriminants -- the 24 bytes is the max size
 * for the Custom(SmolStr) variant. This is acceptable: TagName is used as
 * a HashMap key in StyleIndex, so we need the full value for lookup.
 *
 * id_index: index into CascadeView::idents for the id SelectorIdent.
 * u32::MAX means no id attribute. This avoids storing Option<SelectorIdent>
 * inline (which would be 40 bytes and blow the cache line budget).
 *
 * class_start + class_count: slice into CascadeView::idents for class
 * SelectorIdents. Contiguous storage means the prefetcher can stream
 * class idents linearly during the class lookup loop.
 */
pub struct CascadeEntry {
    pub tag: TagName,
    pub id_index: u32,
    pub class_start: u32,
    pub class_count: u16,
    /// Parent node index (NodeId.raw() as u16). NO_PARENT = no parent.
    /// Enables combinator tree walking without dom.parent() (168-byte fetch).
    /// u16 supports DOMs up to 65534 nodes; larger DOMs fall back to dom.parent().
    pub parent_id: u16,
}

/// Sentinel value for CascadeEntry::id_index when no id attribute exists.
pub const NO_ID: u32 = u32::MAX;

/// Sentinel value for CascadeEntry::parent_id when no parent exists.
pub const NO_PARENT: u16 = u16::MAX;

/*
 * CascadeView -- materialized SoA view of DOM cascade data.
 *
 * entries[i] corresponds to NodeId with raw index i. Non-element nodes
 * (Document, Text, Comment) get a default entry with no tag, no id, no classes.
 *
 * idents is a flat array of all SelectorIdent values (id + class) for all
 * nodes. Each CascadeEntry points into this array via id_index and
 * class_start + class_count. Pre-constructed at materialization time so
 * the cascade hot path never calls SelectorIdent::new_with_atom().
 *
 * rebuild() clears and refills from the DOM, reusing allocated capacity.
 */
pub struct CascadeView {
    pub entries: Vec<CascadeEntry>,
    pub idents: Vec<SelectorIdent>,
}

impl CascadeView {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            idents: Vec::new(),
        }
    }

    /*
     * rebuild -- materialize cascade data from the DOM.
     *
     * WHY: Called once per render (after parse or after mutation batch).
     * Iterates all nodes, extracts tag/id/class into compact SoA layout.
     * Pre-constructs SelectorIdent values so cascade_for_node never does.
     *
     * Cost: O(N * avg_attrs) where N = node count. For 61 nodes with
     * ~1.5 attrs each: ~91 attribute reads. One-time cost amortized over
     * the entire cascade traversal.
     *
     * After rebuild(), entries[node_id.raw()] and idents[start..start+count]
     * are valid for all nodes in the DOM.
     */
    pub fn rebuild(&mut self, dom: &Dom) {
        self.entries.clear();
        self.idents.clear();

        let node_count = dom.node_count();
        // Pre-size entries to node_count, filling non-element nodes with defaults.
        self.entries
            .reserve(node_count.saturating_sub(self.entries.capacity()));

        for idx in 0..node_count {
            let id = NodeId::from_raw(idx);
            // Compute parent_id for combinator tree walking.
            // u16 supports DOMs up to 65534 nodes; larger -> NO_PARENT (fallback).
            let parent_id = dom
                .parent(id)
                .ok()
                .flatten()
                .map(|p| {
                    let raw = p.raw();
                    if raw < u16::MAX as usize {
                        raw as u16
                    } else {
                        NO_PARENT
                    }
                })
                .unwrap_or(NO_PARENT);

            let Ok(node) = dom.node(id) else {
                self.entries.push(CascadeEntry {
                    tag: TagName::Div,
                    id_index: NO_ID,
                    class_start: 0,
                    class_count: 0,
                    parent_id,
                });
                continue;
            };

            let NodeKind::Element {
                name, attributes, ..
            } = node.kind()
            else {
                self.entries.push(CascadeEntry {
                    tag: TagName::Div,
                    id_index: NO_ID,
                    class_start: 0,
                    class_count: 0,
                    parent_id,
                });
                continue;
            };

            let tag = name.clone();
            let mut id_index = NO_ID;

            // Pass 1: push id ident (if any) at a known position.
            for attr in attributes {
                if attr.name == AttributeName::Id {
                    if let Some(atom) = attr.value_atom {
                        id_index = self.idents.len() as u32;
                        self.idents
                            .push(SelectorIdent::new_with_atom(attr.value.clone(), atom));
                    } else if !attr.value.is_empty() {
                        id_index = self.idents.len() as u32;
                        self.idents.push(SelectorIdent::from(attr.value.clone()));
                    }
                    break; // at most one id attribute
                }
            }

            // Pass 2: push class idents contiguously.
            let class_start = self.idents.len() as u32;
            for attr in attributes {
                if attr.name != AttributeName::Class {
                    continue;
                }
                if !attr.class_strings.is_empty() {
                    for (s, &atom) in attr.class_strings.iter().zip(attr.value_atoms.iter()) {
                        self.idents
                            .push(SelectorIdent::new_with_atom(s.clone(), atom));
                    }
                } else if !attr.value_atoms.is_empty() {
                    for &atom in &attr.value_atoms {
                        self.idents.push(SelectorIdent::new_with_atom(
                            dom.resolve_fast(atom).clone(),
                            atom,
                        ));
                    }
                } else {
                    for part in attr.value.as_str().split_whitespace() {
                        self.idents.push(SelectorIdent::new(part));
                    }
                }
            }
            let class_count = self.idents.len() as u32 - class_start;

            self.entries.push(CascadeEntry {
                tag,
                id_index,
                class_start,
                class_count: class_count.min(u16::MAX as u32) as u16,
                parent_id,
            });
        }
    }

    /// Number of entries (= number of nodes in the DOM).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Class idents slice for a given entry.
    #[inline]
    pub fn class_idents(&self, entry: &CascadeEntry) -> &[SelectorIdent] {
        let start = entry.class_start as usize;
        let end = start + entry.class_count as usize;
        &self.idents[start..end]
    }

    /// Id ident for a given entry, if present.
    #[inline]
    pub fn id_ident(&self, entry: &CascadeEntry) -> Option<&SelectorIdent> {
        if entry.id_index != NO_ID {
            Some(&self.idents[entry.id_index as usize])
        } else {
            None
        }
    }

    /// Parent NodeId for a given entry, if present and within u16 range.
    /// Returns None for root nodes or nodes with parent_id > u16::MAX-1.
    #[inline]
    pub fn parent_of(&self, entry: &CascadeEntry) -> Option<NodeId> {
        if entry.parent_id != NO_PARENT {
            Some(NodeId::from_raw(entry.parent_id as usize))
        } else {
            None
        }
    }
}

impl Default for CascadeView {
    fn default() -> Self {
        Self::new()
    }
}
