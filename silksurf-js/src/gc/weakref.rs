//! `WeakRef` and `FinalizationRegistry` Support
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    WEAK REFERENCES                          │
//! ├─────────────────────────────────────────────────────────────┤
//! │                                                             │
//! │  WeakRef:                                                   │
//! │    • Does NOT keep target alive                             │
//! │    • Returns undefined if target collected                  │
//! │    • Cleared during GC sweep phase                          │
//! │                                                             │
//! │  FinalizationRegistry:                                      │
//! │    • Registers callbacks for object cleanup                 │
//! │    • Callbacks run AFTER GC completes                       │
//! │    • Held value passed to callback                          │
//! │    • Can unregister with token                              │
//! │                                                             │
//! │  Implementation:                                            │
//! │    WeakTable ──→ tracks all weak refs by target             │
//! │    FinalizerQueue ──→ pending callbacks to run              │
//! │    Sweep phase ──→ clears dead refs, queues finalizers      │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # ES2021 Semantics
//!
//! - `WeakRef.prototype.deref()` returns target or undefined
//! - Finalization callbacks are non-deterministic (run "eventually")
//! - Unregistered targets don't trigger callbacks

use std::collections::HashMap;

use super::GcRef;

// ============================================================================
// Weak Reference Entry
// ============================================================================

/// A weak reference to a GC-managed object
///
/// The target is cleared to None when the referenced object is collected.
#[derive(Debug, Clone)]
pub struct WeakEntry {
    /// Target object (None if collected)
    target: Option<GcRef>,
    /// Unique ID for this weak ref
    id: u64,
}

impl WeakEntry {
    /// Create a new weak reference to a target
    #[must_use]
    pub fn new(target: GcRef, id: u64) -> Self {
        Self {
            target: Some(target),
            id,
        }
    }

    /// Dereference: get the target if still alive
    #[must_use]
    pub fn deref(&self) -> Option<GcRef> {
        self.target
    }

    /// Check if the target has been collected
    #[must_use]
    pub fn is_dead(&self) -> bool {
        self.target.is_none()
    }

    /// Clear the reference (called during GC)
    pub fn clear(&mut self) {
        self.target = None;
    }

    /// Get unique ID
    #[must_use]
    pub fn id(&self) -> u64 {
        self.id
    }
}

// ============================================================================
// Weak Table
// ============================================================================

/// Table tracking all weak references by their target
///
/// During GC sweep, we iterate this table to clear dead references.
#[derive(Debug, Default)]
pub struct WeakTable {
    /// All weak entries indexed by their ID
    entries: HashMap<u64, WeakEntry>,
    /// Next ID to assign
    next_id: u64,
    /// Index from target pointer to entry IDs (for fast lookup during sweep)
    target_index: HashMap<usize, Vec<u64>>,
}

impl WeakTable {
    /// Create a new empty weak table
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            next_id: 0,
            target_index: HashMap::new(),
        }
    }

    /// Register a new weak reference
    pub fn register(&mut self, target: GcRef) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        let entry = WeakEntry::new(target, id);
        self.entries.insert(id, entry);

        // Index by target pointer for fast sweep lookup
        let target_ptr = target.as_ptr() as usize;
        self.target_index.entry(target_ptr).or_default().push(id);

        id
    }

    /// Dereference a weak reference by ID
    #[must_use]
    pub fn deref(&self, id: u64) -> Option<GcRef> {
        self.entries.get(&id).and_then(WeakEntry::deref)
    }

    /// Remove a weak reference
    pub fn unregister(&mut self, id: u64) {
        if let Some(entry) = self.entries.remove(&id) {
            if let Some(target) = entry.target {
                let target_ptr = target.as_ptr() as usize;
                if let Some(ids) = self.target_index.get_mut(&target_ptr) {
                    ids.retain(|&i| i != id);
                    if ids.is_empty() {
                        self.target_index.remove(&target_ptr);
                    }
                }
            }
        }
    }

    /// Clear all weak references pointing to a collected target
    ///
    /// Called during GC sweep for each WHITE object.
    /// Returns the IDs of cleared references.
    pub fn clear_target(&mut self, target: GcRef) -> Vec<u64> {
        let target_ptr = target.as_ptr() as usize;
        let ids = self.target_index.remove(&target_ptr).unwrap_or_default();

        for &id in &ids {
            if let Some(entry) = self.entries.get_mut(&id) {
                entry.clear();
            }
        }

        ids
    }

    /// Number of registered weak references
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Clean up dead entries (optional compaction)
    pub fn compact(&mut self) {
        self.entries.retain(|_, entry| !entry.is_dead());
    }
}

// ============================================================================
// Finalization Registry Entry
// ============================================================================

/// Callback type for finalization
///
/// In a real implementation, this would be a JavaScript function reference.
/// For now, we use a simple Rust closure type.
pub type FinalizerCallback = Box<dyn FnOnce(HeldValue) + Send>;

/// Held value passed to finalizer callback
///
/// This can be any value the user wants to associate with the registration.
#[derive(Debug, Clone, Default)]
pub enum HeldValue {
    /// No held value
    #[default]
    None,
    /// Integer value
    Integer(i64),
    /// String value
    String(String),
    /// GC reference (must remain alive)
    GcRef(GcRef),
}

/// Registration entry in a finalization registry
#[derive(Debug)]
pub struct FinalizationEntry {
    /// Target object being monitored
    target: GcRef,
    /// Value to pass to callback when target is collected
    held_value: HeldValue,
    /// Unregister token (if provided)
    unregister_token: Option<UnregisterToken>,
    /// Unique ID
    id: u64,
}

impl FinalizationEntry {
    /// Create a new finalization entry
    #[must_use]
    pub fn new(
        target: GcRef,
        held_value: HeldValue,
        unregister_token: Option<UnregisterToken>,
        id: u64,
    ) -> Self {
        Self {
            target,
            held_value,
            unregister_token,
            id,
        }
    }

    /// Get the target
    #[must_use]
    pub fn target(&self) -> GcRef {
        self.target
    }

    /// Get the held value
    #[must_use]
    pub fn held_value(&self) -> &HeldValue {
        &self.held_value
    }

    /// Take the held value (for passing to callback)
    pub fn take_held_value(&mut self) -> HeldValue {
        std::mem::take(&mut self.held_value)
    }
}

// ============================================================================
// Unregister Token
// ============================================================================

/// Token for unregistering finalization callbacks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UnregisterToken(u64);

impl UnregisterToken {
    /// Create a new token
    #[must_use]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// Get token ID
    #[must_use]
    pub const fn id(&self) -> u64 {
        self.0
    }
}

// ============================================================================
// Finalization Registry
// ============================================================================

/// Registry for tracking objects and running cleanup callbacks
///
/// Implements ES2021 `FinalizationRegistry` semantics.
#[derive(Debug, Default)]
pub struct FinalizationRegistry {
    /// All registered entries indexed by ID
    entries: HashMap<u64, FinalizationEntry>,
    /// Index from target pointer to entry IDs
    target_index: HashMap<usize, Vec<u64>>,
    /// Index from unregister token to entry IDs
    token_index: HashMap<u64, Vec<u64>>,
    /// Next ID
    next_id: u64,
    /// Next token ID
    next_token_id: u64,
}

impl FinalizationRegistry {
    /// Create a new finalization registry
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            target_index: HashMap::new(),
            token_index: HashMap::new(),
            next_id: 0,
            next_token_id: 0,
        }
    }

    /// Generate a new unregister token
    pub fn create_token(&mut self) -> UnregisterToken {
        let id = self.next_token_id;
        self.next_token_id += 1;
        UnregisterToken::new(id)
    }

    /// Register an object for finalization
    pub fn register(
        &mut self,
        target: GcRef,
        held_value: HeldValue,
        unregister_token: Option<UnregisterToken>,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        let entry = FinalizationEntry::new(target, held_value, unregister_token, id);
        self.entries.insert(id, entry);

        // Index by target
        let target_ptr = target.as_ptr() as usize;
        self.target_index.entry(target_ptr).or_default().push(id);

        // Index by token if provided
        if let Some(token) = unregister_token {
            self.token_index.entry(token.id()).or_default().push(id);
        }

        id
    }

    /// Unregister all entries with a given token
    pub fn unregister(&mut self, token: UnregisterToken) -> usize {
        let ids = self.token_index.remove(&token.id()).unwrap_or_default();
        let count = ids.len();

        for id in ids {
            if let Some(entry) = self.entries.remove(&id) {
                let target_ptr = entry.target.as_ptr() as usize;
                if let Some(target_ids) = self.target_index.get_mut(&target_ptr) {
                    target_ids.retain(|&i| i != id);
                    if target_ids.is_empty() {
                        self.target_index.remove(&target_ptr);
                    }
                }
            }
        }

        count
    }

    /// Get entries for a collected target
    ///
    /// Called during GC sweep for each WHITE object.
    /// Returns the held values that need finalization callbacks.
    pub fn collect_target(&mut self, target: GcRef) -> Vec<HeldValue> {
        let target_ptr = target.as_ptr() as usize;
        let ids = self.target_index.remove(&target_ptr).unwrap_or_default();

        let mut held_values = Vec::with_capacity(ids.len());

        for id in ids {
            if let Some(mut entry) = self.entries.remove(&id) {
                held_values.push(entry.take_held_value());

                // Clean up token index
                if let Some(token) = entry.unregister_token {
                    if let Some(token_ids) = self.token_index.get_mut(&token.id()) {
                        token_ids.retain(|&i| i != id);
                        if token_ids.is_empty() {
                            self.token_index.remove(&token.id());
                        }
                    }
                }
            }
        }

        held_values
    }

    /// Number of registered entries
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ============================================================================
// Finalizer Queue
// ============================================================================

/// Queue of pending finalization callbacks
///
/// Callbacks are queued during GC and executed afterward.
#[derive(Debug, Default)]
pub struct FinalizerQueue {
    /// Pending held values to process
    pending: Vec<HeldValue>,
}

impl FinalizerQueue {
    /// Create a new empty queue
    #[must_use]
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
        }
    }

    /// Queue a held value for finalization
    pub fn enqueue(&mut self, held_value: HeldValue) {
        self.pending.push(held_value);
    }

    /// Queue multiple held values
    pub fn enqueue_all(&mut self, held_values: impl IntoIterator<Item = HeldValue>) {
        self.pending.extend(held_values);
    }

    /// Drain all pending items
    pub fn drain(&mut self) -> impl Iterator<Item = HeldValue> + '_ {
        self.pending.drain(..)
    }

    /// Number of pending items
    #[must_use]
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// Check if empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Clear all pending items without processing
    pub fn clear(&mut self) {
        self.pending.clear();
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::{Heap, TypeTag};

    #[test]
    fn test_weak_entry() {
        let mut heap = Heap::new();
        let obj = heap.alloc(TypeTag::Object, 32).unwrap();

        let mut entry = WeakEntry::new(obj, 0);
        assert!(!entry.is_dead());
        assert!(entry.deref().is_some());

        entry.clear();
        assert!(entry.is_dead());
        assert!(entry.deref().is_none());
    }

    #[test]
    fn test_weak_table_register() {
        let mut heap = Heap::new();
        let obj = heap.alloc(TypeTag::Object, 32).unwrap();

        let mut table = WeakTable::new();
        let id = table.register(obj);

        assert_eq!(table.len(), 1);
        assert!(table.deref(id).is_some());
    }

    #[test]
    fn test_weak_table_clear_target() {
        let mut heap = Heap::new();
        let obj = heap.alloc(TypeTag::Object, 32).unwrap();

        let mut table = WeakTable::new();
        let id1 = table.register(obj);
        let id2 = table.register(obj); // Two refs to same target

        let cleared = table.clear_target(obj);
        assert_eq!(cleared.len(), 2);
        assert!(cleared.contains(&id1));
        assert!(cleared.contains(&id2));

        // References should now be dead
        assert!(table.deref(id1).is_none());
        assert!(table.deref(id2).is_none());
    }

    #[test]
    fn test_weak_table_unregister() {
        let mut heap = Heap::new();
        let obj = heap.alloc(TypeTag::Object, 32).unwrap();

        let mut table = WeakTable::new();
        let id = table.register(obj);

        table.unregister(id);
        assert!(table.deref(id).is_none());
        assert_eq!(table.len(), 0);
    }

    #[test]
    fn test_finalization_registry_register() {
        let mut heap = Heap::new();
        let obj = heap.alloc(TypeTag::Object, 32).unwrap();

        let mut registry = FinalizationRegistry::new();
        let id = registry.register(obj, HeldValue::Integer(42), None);

        assert_eq!(registry.len(), 1);
        assert!(id < registry.next_id);
    }

    #[test]
    fn test_finalization_registry_collect() {
        let mut heap = Heap::new();
        let obj = heap.alloc(TypeTag::Object, 32).unwrap();

        let mut registry = FinalizationRegistry::new();
        registry.register(obj, HeldValue::Integer(42), None);
        registry.register(obj, HeldValue::String("cleanup".to_string()), None);

        let held_values = registry.collect_target(obj);
        assert_eq!(held_values.len(), 2);
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn test_finalization_registry_unregister() {
        let mut heap = Heap::new();
        let obj = heap.alloc(TypeTag::Object, 32).unwrap();

        let mut registry = FinalizationRegistry::new();
        let token = registry.create_token();
        registry.register(obj, HeldValue::Integer(1), Some(token));
        registry.register(obj, HeldValue::Integer(2), Some(token));
        registry.register(obj, HeldValue::Integer(3), None); // No token

        assert_eq!(registry.len(), 3);

        let unregistered = registry.unregister(token);
        assert_eq!(unregistered, 2);
        assert_eq!(registry.len(), 1); // Only the one without token remains
    }

    #[test]
    fn test_finalizer_queue() {
        let mut queue = FinalizerQueue::new();

        queue.enqueue(HeldValue::Integer(1));
        queue.enqueue(HeldValue::Integer(2));
        queue.enqueue_all(vec![HeldValue::Integer(3), HeldValue::Integer(4)]);

        assert_eq!(queue.len(), 4);

        let items: Vec<_> = queue.drain().collect();
        assert_eq!(items.len(), 4);
        assert!(queue.is_empty());
    }

    #[test]
    fn test_held_value_variants() {
        let none = HeldValue::None;
        let int = HeldValue::Integer(42);
        let string = HeldValue::String("test".to_string());

        assert!(matches!(none, HeldValue::None));
        assert!(matches!(int, HeldValue::Integer(42)));
        assert!(matches!(string, HeldValue::String(s) if s == "test"));
    }
}
