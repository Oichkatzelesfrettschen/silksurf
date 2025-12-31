//! GC Integration for the Virtual Machine
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    VM-GC INTEGRATION                        │
//! ├─────────────────────────────────────────────────────────────┤
//! │                                                             │
//! │  ┌─────────┐    roots     ┌─────────┐                      │
//! │  │   VM    │──────────────│   GC    │                      │
//! │  │         │              │  Heap   │                      │
//! │  │ regs    │◄─────────────│         │                      │
//! │  │ stack   │   GcRef      │ objects │                      │
//! │  │ global  │              │ arrays  │                      │
//! │  └─────────┘              │ strings │                      │
//! │                           └─────────┘                      │
//! │                                                             │
//! │  Root Sources:                                              │
//! │    1. Register file (values containing GcRefs)              │
//! │    2. Call stack (closures, environments)                   │
//! │    3. Global object                                         │
//! │    4. Native handles (callbacks, exports)                   │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Phases of Integration
//!
//! 1. **Value Bridge**: Connect `Value` variants to `GcRef`
//! 2. **Root Enumeration**: Walk VM state to find GC roots
//! 3. **Allocation Path**: Route object creation through heap
//! 4. **Collection Trigger**: Invoke GC at safe points

use crate::gc::{
    FinalizationRegistry, FinalizerQueue, GcRef, HeldValue, Heap, RootSet, Trace, Tracer,
    UnregisterToken, WeakTable,
};

// ============================================================================
// Marking Tracer (Integrates with Heap)
// ============================================================================

/// Tracer that marks objects reachable in the heap
///
/// Used during GC mark phase to traverse the object graph.
pub struct MarkingTracer<'a> {
    /// Reference to the heap for marking
    heap: &'a mut Heap,
}

impl<'a> MarkingTracer<'a> {
    /// Create a new marking tracer
    pub fn new(heap: &'a mut Heap) -> Self {
        Self { heap }
    }
}

impl Tracer for MarkingTracer<'_> {
    fn visit(&mut self, reference: GcRef) {
        self.heap.mark(reference);
    }
}

// ============================================================================
// GC Value (Future Bridge Type)
// ============================================================================

/// Value that may contain GC references
///
/// This is a bridge type for tracking which VM values contain heap references.
/// In a full implementation, this would replace `Value` or augment it.
#[derive(Debug, Clone, Copy)]
pub enum GcValue {
    /// No GC reference (primitives)
    None,
    /// Direct GC reference
    Ref(GcRef),
}

impl GcValue {
    /// Check if this value contains a GC reference
    #[must_use]
    pub const fn has_ref(&self) -> bool {
        matches!(self, Self::Ref(_))
    }

    /// Get the GC reference if present
    #[must_use]
    pub const fn get_ref(&self) -> Option<GcRef> {
        match self {
            Self::None => None,
            Self::Ref(r) => Some(*r),
        }
    }
}

impl Trace for GcValue {
    fn trace(&self, tracer: &mut dyn Tracer) {
        if let Self::Ref(r) = self {
            tracer.visit(*r);
        }
    }
}

// ============================================================================
// VM GC State
// ============================================================================

/// GC state attached to a VM
///
/// Manages heap allocation and collection for a single VM instance.
#[derive(Debug)]
pub struct VmGcState {
    /// The managed heap
    pub heap: Heap,
    /// Root set for external references
    pub roots: RootSet,
    /// GC-tracked values (temporary until full Value integration)
    tracked_values: Vec<GcValue>,
    /// Weak reference table
    weak_table: WeakTable,
    /// Finalization registry
    finalization_registry: FinalizationRegistry,
    /// Queue of pending finalizers
    finalizer_queue: FinalizerQueue,
}

impl VmGcState {
    /// Create new GC state
    #[must_use]
    pub fn new() -> Self {
        Self {
            heap: Heap::new(),
            roots: RootSet::new(),
            tracked_values: Vec::new(),
            weak_table: WeakTable::new(),
            finalization_registry: FinalizationRegistry::new(),
            finalizer_queue: FinalizerQueue::new(),
        }
    }

    /// Track a GC value
    pub fn track(&mut self, value: GcValue) -> usize {
        let idx = self.tracked_values.len();
        self.tracked_values.push(value);
        idx
    }

    /// Get a tracked value
    #[must_use]
    pub fn get_tracked(&self, idx: usize) -> Option<GcValue> {
        self.tracked_values.get(idx).copied()
    }

    /// Add an external root
    pub fn add_root(&mut self, root: GcRef) {
        self.roots.add(root);
    }

    /// Remove an external root
    pub fn remove_root(&mut self, root: GcRef) {
        self.roots.remove(root);
    }

    // ========================================================================
    // Weak Reference API
    // ========================================================================

    /// Create a weak reference to an object
    pub fn create_weak_ref(&mut self, target: GcRef) -> u64 {
        self.weak_table.register(target)
    }

    /// Dereference a weak reference
    ///
    /// Returns None if the target has been collected.
    #[must_use]
    pub fn deref_weak(&self, weak_id: u64) -> Option<GcRef> {
        self.weak_table.deref(weak_id)
    }

    /// Remove a weak reference
    pub fn remove_weak_ref(&mut self, weak_id: u64) {
        self.weak_table.unregister(weak_id);
    }

    // ========================================================================
    // Finalization Registry API
    // ========================================================================

    /// Create an unregister token
    pub fn create_unregister_token(&mut self) -> UnregisterToken {
        self.finalization_registry.create_token()
    }

    /// Register an object for finalization
    ///
    /// When `target` is collected, `held_value` will be passed to finalization.
    pub fn register_finalizer(
        &mut self,
        target: GcRef,
        held_value: HeldValue,
        unregister_token: Option<UnregisterToken>,
    ) -> u64 {
        self.finalization_registry
            .register(target, held_value, unregister_token)
    }

    /// Unregister all finalizers with the given token
    pub fn unregister_finalizers(&mut self, token: UnregisterToken) -> usize {
        self.finalization_registry.unregister(token)
    }

    /// Check if there are pending finalizers
    #[must_use]
    pub fn has_pending_finalizers(&self) -> bool {
        !self.finalizer_queue.is_empty()
    }

    /// Drain pending finalizers
    ///
    /// Returns held values that need finalization callbacks.
    pub fn drain_finalizers(&mut self) -> Vec<HeldValue> {
        self.finalizer_queue.drain().collect()
    }

    // ========================================================================
    // GC Operations
    // ========================================================================

    /// Check if GC should run
    #[must_use]
    pub fn should_collect(&self) -> bool {
        self.heap.should_collect()
    }

    /// Run garbage collection
    ///
    /// Enumerates all roots from tracked values and external roots,
    /// then performs mark-sweep collection. Clears weak references
    /// and queues finalizers for collected objects.
    pub fn collect(&mut self) {
        // Collect objects to process (we can't borrow self in the callback)
        let mut collected_objects = Vec::new();

        {
            let tracked = &self.tracked_values;
            let external = &self.roots;

            self.heap.collect_with_callback(
                |mark| {
                    // Mark tracked values
                    for value in tracked {
                        if let GcValue::Ref(r) = value {
                            mark(*r);
                        }
                    }
                    // Mark external roots
                    for root in external.iter() {
                        mark(root);
                    }
                },
                |collected| {
                    // Collect references for post-processing
                    collected_objects.push(collected);
                },
            );
        }

        // Process collected objects: clear weak refs, queue finalizers
        for obj in collected_objects {
            // Clear weak references to this object
            self.weak_table.clear_target(obj);

            // Queue finalizers for this object
            let held_values = self.finalization_registry.collect_target(obj);
            self.finalizer_queue.enqueue_all(held_values);
        }
    }

    /// Process weak references for a collected object
    ///
    /// Called during sweep for each WHITE object before it's freed.
    pub fn process_collected_object(&mut self, collected: GcRef) {
        // Clear weak references to this object
        self.weak_table.clear_target(collected);

        // Queue finalizers for this object
        let held_values = self.finalization_registry.collect_target(collected);
        self.finalizer_queue.enqueue_all(held_values);
    }

    /// Compact weak table (remove dead entries)
    pub fn compact_weak_table(&mut self) {
        self.weak_table.compact();
    }

    /// Get heap statistics
    #[must_use]
    pub fn stats(&self) -> &crate::gc::GcStats {
        self.heap.stats()
    }

    /// Number of weak references
    #[must_use]
    pub fn weak_ref_count(&self) -> usize {
        self.weak_table.len()
    }

    /// Number of registered finalizers
    #[must_use]
    pub fn finalizer_count(&self) -> usize {
        self.finalization_registry.len()
    }
}

impl Default for VmGcState {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// VM Root Enumeration Protocol
// ============================================================================

/// Trait for types that can enumerate their GC roots
pub trait EnumerateRoots {
    /// Enumerate all GC roots in this object
    fn enumerate_roots(&self, callback: &mut dyn FnMut(GcRef));
}

impl EnumerateRoots for RootSet {
    fn enumerate_roots(&self, callback: &mut dyn FnMut(GcRef)) {
        for root in self.iter() {
            callback(root);
        }
    }
}

impl EnumerateRoots for Vec<GcValue> {
    fn enumerate_roots(&self, callback: &mut dyn FnMut(GcRef)) {
        for value in self {
            if let GcValue::Ref(r) = value {
                callback(*r);
            }
        }
    }
}

// ============================================================================
// Safe Points
// ============================================================================

/// Marker for GC safe points
///
/// Safe points are locations in execution where GC can safely run:
/// - Between bytecode instructions
/// - At function calls/returns
/// - At loop back-edges
/// - At allocation sites
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafePoint {
    /// Between instructions
    Instruction,
    /// At function boundary
    FunctionBoundary,
    /// At loop back-edge
    LoopBackEdge,
    /// At allocation site
    Allocation,
}

/// Check if we should collect at this safe point
#[must_use]
pub fn should_collect_at(gc: &VmGcState, _safe_point: SafePoint) -> bool {
    // For now, simple threshold check at any safe point
    // Future: could have different thresholds for different safe points
    gc.should_collect()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::TypeTag;

    #[test]
    fn test_vm_gc_state_creation() {
        let gc = VmGcState::new();
        assert_eq!(gc.stats().objects_allocated, 0);
        assert!(gc.roots.is_empty());
    }

    #[test]
    fn test_track_gc_value() {
        let mut gc = VmGcState::new();

        let obj = gc.heap.alloc(TypeTag::Object, 32).unwrap();
        let idx = gc.track(GcValue::Ref(obj));

        assert_eq!(idx, 0);
        assert!(gc.get_tracked(0).is_some());
        assert!(gc.get_tracked(0).unwrap().has_ref());
    }

    #[test]
    fn test_external_roots() {
        let mut gc = VmGcState::new();

        let obj = gc.heap.alloc(TypeTag::Object, 32).unwrap();
        gc.add_root(obj);
        assert_eq!(gc.roots.len(), 1);

        gc.remove_root(obj);
        assert!(gc.roots.is_empty());
    }

    #[test]
    fn test_gc_collect_preserves_roots() {
        let mut gc = VmGcState::new();

        // Allocate objects
        let obj1 = gc.heap.alloc(TypeTag::Object, 32).unwrap();
        let obj2 = gc.heap.alloc(TypeTag::Object, 32).unwrap();
        let _obj3 = gc.heap.alloc(TypeTag::Object, 32).unwrap(); // Not rooted

        // Track obj1, root obj2
        gc.track(GcValue::Ref(obj1));
        gc.add_root(obj2);

        assert_eq!(gc.stats().objects_allocated, 3);

        // Run GC
        gc.collect();

        // obj3 should be collected
        assert_eq!(gc.stats().objects_collected, 1);
    }

    #[test]
    fn test_gc_value_trace() {
        let mut heap = Heap::new();
        let obj = heap.alloc(TypeTag::Object, 32).unwrap();

        let value = GcValue::Ref(obj);

        let mut tracer = crate::gc::CountingTracer::new();
        value.trace(&mut tracer);

        assert_eq!(tracer.count, 1);
    }

    #[test]
    fn test_marking_tracer() {
        let mut heap = Heap::new();
        let obj = heap.alloc(TypeTag::Object, 32).unwrap();

        {
            let mut tracer = MarkingTracer::new(&mut heap);
            tracer.visit(obj);
        }

        // After marking, the object should be gray
        // (would need to expose color for this check)
    }

    #[test]
    fn test_weak_ref_creation() {
        let mut gc = VmGcState::new();

        let obj = gc.heap.alloc(TypeTag::Object, 32).unwrap();
        let weak_id = gc.create_weak_ref(obj);

        assert_eq!(gc.weak_ref_count(), 1);
        assert!(gc.deref_weak(weak_id).is_some());
    }

    #[test]
    fn test_weak_ref_removal() {
        let mut gc = VmGcState::new();

        let obj = gc.heap.alloc(TypeTag::Object, 32).unwrap();
        let weak_id = gc.create_weak_ref(obj);

        gc.remove_weak_ref(weak_id);
        assert_eq!(gc.weak_ref_count(), 0);
        assert!(gc.deref_weak(weak_id).is_none());
    }

    #[test]
    fn test_weak_ref_cleared_on_collect() {
        let mut gc = VmGcState::new();

        let obj = gc.heap.alloc(TypeTag::Object, 32).unwrap();
        let weak_id = gc.create_weak_ref(obj);

        // Simulate collection of the object
        gc.process_collected_object(obj);

        // Weak ref should now be dead
        assert!(gc.deref_weak(weak_id).is_none());
    }

    #[test]
    fn test_finalization_registry_basic() {
        let mut gc = VmGcState::new();

        let obj = gc.heap.alloc(TypeTag::Object, 32).unwrap();
        gc.register_finalizer(obj, HeldValue::Integer(42), None);

        assert_eq!(gc.finalizer_count(), 1);
        assert!(!gc.has_pending_finalizers());
    }

    #[test]
    fn test_finalization_queued_on_collect() {
        let mut gc = VmGcState::new();

        let obj = gc.heap.alloc(TypeTag::Object, 32).unwrap();
        gc.register_finalizer(obj, HeldValue::Integer(42), None);
        gc.register_finalizer(obj, HeldValue::String("cleanup".to_string()), None);

        // Simulate collection
        gc.process_collected_object(obj);

        // Finalizers should be queued
        assert!(gc.has_pending_finalizers());
        let pending = gc.drain_finalizers();
        assert_eq!(pending.len(), 2);
        assert!(!gc.has_pending_finalizers());
    }

    #[test]
    fn test_finalization_unregister() {
        let mut gc = VmGcState::new();

        let obj = gc.heap.alloc(TypeTag::Object, 32).unwrap();
        let token = gc.create_unregister_token();

        gc.register_finalizer(obj, HeldValue::Integer(1), Some(token));
        gc.register_finalizer(obj, HeldValue::Integer(2), Some(token));
        gc.register_finalizer(obj, HeldValue::Integer(3), None);

        assert_eq!(gc.finalizer_count(), 3);

        let unregistered = gc.unregister_finalizers(token);
        assert_eq!(unregistered, 2);
        assert_eq!(gc.finalizer_count(), 1);
    }

    #[test]
    fn test_weak_table_compact() {
        let mut gc = VmGcState::new();

        let obj = gc.heap.alloc(TypeTag::Object, 32).unwrap();
        gc.create_weak_ref(obj);
        gc.create_weak_ref(obj);

        assert_eq!(gc.weak_ref_count(), 2);

        // Clear the weak refs
        gc.process_collected_object(obj);

        // They're still in the table (but dead)
        assert_eq!(gc.weak_ref_count(), 2);

        // Compact removes dead entries
        gc.compact_weak_table();
        assert_eq!(gc.weak_ref_count(), 0);
    }

    #[test]
    fn test_gc_cycle_clears_weak_refs() {
        let mut gc = VmGcState::new();

        // Allocate objects: obj1 is rooted, obj2 is not
        let obj1 = gc.heap.alloc(TypeTag::Object, 32).unwrap();
        let obj2 = gc.heap.alloc(TypeTag::Object, 32).unwrap();

        // Root obj1
        gc.add_root(obj1);

        // Create weak refs to both
        let weak1 = gc.create_weak_ref(obj1);
        let weak2 = gc.create_weak_ref(obj2);

        assert!(gc.deref_weak(weak1).is_some());
        assert!(gc.deref_weak(weak2).is_some());

        // Run GC - obj2 should be collected
        gc.collect();

        // weak1 should still be alive, weak2 should be dead
        assert!(gc.deref_weak(weak1).is_some());
        assert!(gc.deref_weak(weak2).is_none());
    }

    #[test]
    fn test_gc_cycle_queues_finalizers() {
        let mut gc = VmGcState::new();

        // Allocate objects
        let obj1 = gc.heap.alloc(TypeTag::Object, 32).unwrap();
        let obj2 = gc.heap.alloc(TypeTag::Object, 32).unwrap();

        // Root obj1
        gc.add_root(obj1);

        // Register finalizers
        gc.register_finalizer(obj1, HeldValue::Integer(1), None);
        gc.register_finalizer(obj2, HeldValue::Integer(2), None);

        assert!(!gc.has_pending_finalizers());

        // Run GC - obj2 should be collected
        gc.collect();

        // Only obj2's finalizer should be queued
        assert!(gc.has_pending_finalizers());
        let pending = gc.drain_finalizers();
        assert_eq!(pending.len(), 1);
        match &pending[0] {
            HeldValue::Integer(2) => {}
            _ => panic!("Expected Integer(2)"),
        }
    }

    #[test]
    fn test_gc_with_weak_and_finalizer() {
        let mut gc = VmGcState::new();

        // Allocate an unrooted object
        let obj = gc.heap.alloc(TypeTag::Object, 32).unwrap();

        // Create weak ref and register finalizer
        let weak_id = gc.create_weak_ref(obj);
        gc.register_finalizer(obj, HeldValue::String("cleanup".to_string()), None);

        // Object is alive before GC
        assert!(gc.deref_weak(weak_id).is_some());
        assert!(!gc.has_pending_finalizers());

        // Run GC - object should be collected
        gc.collect();

        // Weak ref should be dead
        assert!(gc.deref_weak(weak_id).is_none());

        // Finalizer should be queued
        assert!(gc.has_pending_finalizers());
        let pending = gc.drain_finalizers();
        assert_eq!(pending.len(), 1);
    }
}
