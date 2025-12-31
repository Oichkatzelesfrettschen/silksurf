//! Object Tracing for Garbage Collection
//!
//! # The Trace Abstraction
//!
//! Every GC-managed type must implement `Trace` to enumerate its children.
//! This enables the collector to discover the full object graph:
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    TRACE PROTOCOL                           │
//! ├─────────────────────────────────────────────────────────────┤
//! │  trait Trace {                                              │
//! │      fn trace(&self, tracer: &mut Tracer);                  │
//! │  }                                                          │
//! │                                                             │
//! │  - Called during mark phase for each gray object            │
//! │  - Implementation visits all GC-managed fields              │
//! │  - Tracer.visit() marks children as reachable               │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Safety Model
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    INVARIANTS                               │
//! ├─────────────────────────────────────────────────────────────┤
//! │  1. Trace must visit ALL GcRef fields                       │
//! │  2. Trace must NOT access non-memory resources              │
//! │  3. Trace must NOT allocate or trigger GC                   │
//! │  4. Trace must be deterministic (same fields each call)     │
//! └─────────────────────────────────────────────────────────────┘
//! ```

use super::heap::GcRef;

// ============================================================================
// Tracer (Visitor Pattern)
// ============================================================================

/// Visitor for tracing GC references
///
/// The GC provides this during marking. Objects call `visit()` for each
/// child reference, and the tracer handles marking logic.
pub trait Tracer {
    /// Visit a GC reference (mark it as reachable)
    fn visit(&mut self, reference: GcRef);

    /// Visit an optional GC reference
    fn visit_opt(&mut self, reference: Option<GcRef>) {
        if let Some(r) = reference {
            self.visit(r);
        }
    }

    /// Visit a slice of GC references
    fn visit_slice(&mut self, references: &[GcRef]) {
        for &r in references {
            self.visit(r);
        }
    }
}

// ============================================================================
// Trace Trait
// ============================================================================

/// Trait for types that contain GC references
///
/// # Implementation Guidelines
///
/// 1. Call `tracer.visit()` for each `GcRef` field
/// 2. Recursively trace contained types that impl `Trace`
/// 3. Do NOT skip fields - all references must be visited
///
/// # Example
///
/// ```ignore
/// struct MyObject {
///     name: GcRef,         // String reference
///     children: Vec<GcRef>, // Array of children
///     parent: Option<GcRef>,
/// }
///
/// impl Trace for MyObject {
///     fn trace(&self, tracer: &mut dyn Tracer) {
///         tracer.visit(self.name);
///         tracer.visit_slice(&self.children);
///         tracer.visit_opt(self.parent);
///     }
/// }
/// ```
pub trait Trace {
    /// Enumerate all GC references in this object
    fn trace(&self, tracer: &mut dyn Tracer);
}

// ============================================================================
// Implementations for Standard Types
// ============================================================================

/// Primitive types have no references
impl Trace for () {
    fn trace(&self, _tracer: &mut dyn Tracer) {}
}

impl Trace for bool {
    fn trace(&self, _tracer: &mut dyn Tracer) {}
}

impl Trace for i32 {
    fn trace(&self, _tracer: &mut dyn Tracer) {}
}

impl Trace for i64 {
    fn trace(&self, _tracer: &mut dyn Tracer) {}
}

impl Trace for u32 {
    fn trace(&self, _tracer: &mut dyn Tracer) {}
}

impl Trace for u64 {
    fn trace(&self, _tracer: &mut dyn Tracer) {}
}

impl Trace for f64 {
    fn trace(&self, _tracer: &mut dyn Tracer) {}
}

impl Trace for String {
    fn trace(&self, _tracer: &mut dyn Tracer) {}
}

impl Trace for &str {
    fn trace(&self, _tracer: &mut dyn Tracer) {}
}

/// Option delegates to inner
impl<T: Trace> Trace for Option<T> {
    fn trace(&self, tracer: &mut dyn Tracer) {
        if let Some(inner) = self {
            inner.trace(tracer);
        }
    }
}

/// Vec traces each element
impl<T: Trace> Trace for Vec<T> {
    fn trace(&self, tracer: &mut dyn Tracer) {
        for item in self {
            item.trace(tracer);
        }
    }
}

/// Box delegates to inner
impl<T: Trace + ?Sized> Trace for Box<T> {
    fn trace(&self, tracer: &mut dyn Tracer) {
        (**self).trace(tracer);
    }
}

/// `GcRef` is itself a traceable reference
impl Trace for GcRef {
    fn trace(&self, tracer: &mut dyn Tracer) {
        tracer.visit(*self);
    }
}

// ============================================================================
// Root Set
// ============================================================================

/// Root set for garbage collection
///
/// Collects all entry points into the object graph:
/// - VM registers
/// - Call stack frames
/// - Global object
/// - Native handles
#[derive(Debug, Default)]
pub struct RootSet {
    /// Registered root references
    roots: Vec<GcRef>,
}

impl RootSet {
    /// Create empty root set
    #[must_use]
    pub fn new() -> Self {
        Self { roots: Vec::new() }
    }

    /// Add a root reference
    pub fn add(&mut self, root: GcRef) {
        self.roots.push(root);
    }

    /// Remove a root (if present)
    pub fn remove(&mut self, root: GcRef) {
        self.roots.retain(|&r| r != root);
    }

    /// Clear all roots
    pub fn clear(&mut self) {
        self.roots.clear();
    }

    /// Enumerate all roots
    pub fn iter(&self) -> impl Iterator<Item = GcRef> + '_ {
        self.roots.iter().copied()
    }

    /// Number of roots
    #[must_use]
    pub fn len(&self) -> usize {
        self.roots.len()
    }

    /// Check if empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }
}

impl Trace for RootSet {
    fn trace(&self, tracer: &mut dyn Tracer) {
        tracer.visit_slice(&self.roots);
    }
}

// ============================================================================
// Counting Tracer (for debugging)
// ============================================================================

/// Tracer that counts visits (for testing/debugging)
#[derive(Debug, Default)]
pub struct CountingTracer {
    /// Number of references visited
    pub count: usize,
    /// All visited references
    pub visited: Vec<GcRef>,
}

impl CountingTracer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl Tracer for CountingTracer {
    fn visit(&mut self, reference: GcRef) {
        self.count += 1;
        self.visited.push(reference);
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
    fn test_primitive_trace() {
        let mut tracer = CountingTracer::new();

        // Primitives have no references
        42i32.trace(&mut tracer);
        3.14f64.trace(&mut tracer);
        true.trace(&mut tracer);

        assert_eq!(tracer.count, 0);
    }

    #[test]
    fn test_option_trace() {
        let mut heap = Heap::new();
        let obj = heap.alloc(TypeTag::Object, 32).unwrap();

        let mut tracer = CountingTracer::new();

        let opt_some: Option<GcRef> = Some(obj);
        let opt_none: Option<GcRef> = None;

        opt_some.trace(&mut tracer);
        opt_none.trace(&mut tracer);

        assert_eq!(tracer.count, 1);
    }

    #[test]
    fn test_vec_trace() {
        let mut heap = Heap::new();
        let obj1 = heap.alloc(TypeTag::Object, 32).unwrap();
        let obj2 = heap.alloc(TypeTag::Object, 32).unwrap();
        let obj3 = heap.alloc(TypeTag::Object, 32).unwrap();

        let mut tracer = CountingTracer::new();

        let refs = vec![obj1, obj2, obj3];
        refs.trace(&mut tracer);

        assert_eq!(tracer.count, 3);
    }

    #[test]
    fn test_root_set() {
        let mut heap = Heap::new();
        let obj1 = heap.alloc(TypeTag::Object, 32).unwrap();
        let obj2 = heap.alloc(TypeTag::Object, 32).unwrap();

        let mut roots = RootSet::new();
        roots.add(obj1);
        roots.add(obj2);

        assert_eq!(roots.len(), 2);

        let mut tracer = CountingTracer::new();
        roots.trace(&mut tracer);

        assert_eq!(tracer.count, 2);
    }

    #[test]
    fn test_nested_trace() {
        let mut heap = Heap::new();
        let obj = heap.alloc(TypeTag::Object, 32).unwrap();

        let mut tracer = CountingTracer::new();

        // Nested: Vec<Option<GcRef>>
        let nested: Vec<Option<GcRef>> = vec![Some(obj), None, Some(obj)];
        nested.trace(&mut tracer);

        assert_eq!(tracer.count, 2);
    }
}
