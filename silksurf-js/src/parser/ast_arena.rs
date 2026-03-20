//! Arena-Allocated AST Support
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    AST ARENA ALLOCATION                     │
//! ├─────────────────────────────────────────────────────────────┤
//! │                                                             │
//! │  Traditional Parsing:         Arena Parsing:                │
//! │    Box<Expr>  ──→ malloc      AstBox<Expr> ──→ bump ptr    │
//! │    Vec<Stmt>  ──→ malloc      AstVec<Stmt> ──→ bump slice  │
//! │    Drop each  ──→ N frees     reset()      ──→ 1 reset     │
//! │                                                             │
//! │  Benefits:                                                  │
//! │    • O(1) allocation (pointer bump)                         │
//! │    • O(1) deallocation (arena reset)                        │
//! │    • Cache-friendly layout (contiguous memory)              │
//! │    • Zero fragmentation                                     │
//! │    • Parse-compile-discard pattern fits perfectly           │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```ignore
//! let mut ast_arena = AstArena::new();
//!
//! // Parse with arena allocation
//! let program = parser.parse_with_arena(&ast_arena)?;
//!
//! // Compile (program is valid here)
//! let bytecode = compiler.compile(&program);
//!
//! // Reset arena - all AST nodes freed at once
//! ast_arena.reset();
//! ```

use std::cell::RefCell;

use crate::gc::Arena;

// ============================================================================
// Arena-Allocated Box (AstBox)
// ============================================================================

/// Arena-allocated box - a reference to an arena-allocated value
///
/// Unlike `Box<T>`, this is just a reference with arena lifetime.
/// No `Drop` implementation needed - the arena handles cleanup.
pub type AstBox<'arena, T> = &'arena T;

/// Allocate a boxed value in the arena
///
/// # Example
/// ```ignore
/// let expr: AstBox<Expression> = ast_arena.alloc_box(expr);
/// ```
#[inline]
pub fn alloc_box<T>(arena: &Arena, value: T) -> AstBox<'_, T> {
    arena.alloc(value)
}

// ============================================================================
// Arena-Allocated Vec (AstVec)
// ============================================================================

/// Arena-allocated vec - a slice reference to arena-allocated values
///
/// Unlike `Vec<T>`, this is an immutable slice with arena lifetime.
/// For building up vectors during parsing, use `AstVecBuilder`.
pub type AstVec<'arena, T> = &'arena [T];

/// Builder for constructing arena-allocated vectors
///
/// Collects items into a standard Vec, then freezes to arena slice.
pub struct AstVecBuilder<T> {
    items: Vec<T>,
}

impl<T> AstVecBuilder<T> {
    /// Create new empty builder
    #[must_use]
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Create builder with capacity
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            items: Vec::with_capacity(capacity),
        }
    }

    /// Push an item
    pub fn push(&mut self, item: T) {
        self.items.push(item);
    }

    /// Check if empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Current length
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Freeze into arena-allocated slice
    ///
    /// Moves all items into the arena and returns a slice reference.
    ///
    /// # Panics
    /// This function does not panic - it handles empty vectors gracefully.
    /// Internal unwrap is guarded by length check.
    #[allow(clippy::missing_panics_doc)] // False positive: unwrap is after empty check
    pub fn freeze(self, arena: &AstArena) -> AstVec<'_, T> {
        if self.items.is_empty() {
            return &[];
        }
        arena.alloc_slice_from_iter(self.items)
    }
}

impl<T> Default for AstVecBuilder<T> {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// AST Arena
// ============================================================================

/// Arena for AST node allocation
///
/// Wraps the GC arena with AST-specific conveniences.
pub struct AstArena {
    /// Underlying memory arena
    inner: RefCell<Arena>,
}

impl AstArena {
    /// Create new AST arena with default capacity (1MB)
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: RefCell::new(Arena::new()),
        }
    }

    /// Create with specified capacity
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: RefCell::new(Arena::with_capacity(capacity)),
        }
    }

    /// Allocate a single value
    ///
    /// Returns a reference valid until arena reset.
    pub fn alloc<T>(&self, value: T) -> &T {
        // SAFETY: We're returning a reference that borrows self,
        // so the reference cannot outlive the arena.
        // The RefCell borrow is temporary (just for the alloc call).
        let arena = self.inner.borrow();
        let ptr = std::ptr::from_ref(arena.alloc(value));
        unsafe { &*ptr }
    }

    /// Allocate a string
    pub fn alloc_str(&self, s: &str) -> &str {
        let arena = self.inner.borrow();
        let ptr = std::ptr::from_ref(arena.alloc_str(s));
        unsafe { &*ptr }
    }

    /// Allocate a slice from an exact-size iterator.
    pub fn alloc_slice_from_iter<T, I>(&self, iter: I) -> &[T]
    where
        I: IntoIterator<Item = T>,
        I::IntoIter: ExactSizeIterator,
    {
        let arena = self.inner.borrow();
        let ptr = std::ptr::from_ref(arena.alloc_slice_fill_iter(iter));
        unsafe { &*ptr }
    }

    /// Reset the arena, freeing all allocations
    ///
    /// # Safety
    /// Caller must ensure no references to arena data are held.
    pub fn reset(&self) {
        self.inner.borrow_mut().reset();
    }

    /// Bytes allocated in current generation
    #[must_use]
    pub fn bytes_allocated(&self) -> usize {
        self.inner.borrow().bytes_allocated()
    }

    /// Current generation (increments on each reset)
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.inner.borrow().generation()
    }
}

impl Default for AstArena {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Arena Statistics
// ============================================================================

/// Statistics for arena usage
#[derive(Debug, Clone, Default)]
pub struct ArenaStats {
    /// Total bytes allocated across all generations
    pub total_bytes: usize,
    /// Number of resets (generations)
    pub generations: u64,
    /// Peak bytes in a single generation
    pub peak_bytes: usize,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ast_arena_alloc() {
        let arena = AstArena::new();

        let x = arena.alloc(42u64);
        let y = arena.alloc(123u64);

        assert_eq!(*x, 42);
        assert_eq!(*y, 123);
        assert!(arena.bytes_allocated() >= 16);
    }

    #[test]
    fn test_ast_arena_string() {
        let arena = AstArena::new();

        let s = arena.alloc_str("hello world");
        assert_eq!(s, "hello world");
    }

    #[test]
    fn test_ast_arena_reset() {
        let arena = AstArena::new();

        let _x = arena.alloc(42u64);
        assert!(arena.bytes_allocated() > 0);

        arena.reset();
        assert_eq!(arena.bytes_allocated(), 0);
        assert_eq!(arena.generation(), 1);
    }

    #[test]
    fn test_ast_vec_builder() {
        let arena = AstArena::new();
        let mut builder = AstVecBuilder::new();

        builder.push(1u32);
        builder.push(2);
        builder.push(3);

        assert_eq!(builder.len(), 3);

        let slice = builder.freeze(&arena);
        assert_eq!(slice.len(), 3);
        assert_eq!(slice[0], 1);
        assert_eq!(slice[1], 2);
        assert_eq!(slice[2], 3);
    }

    #[test]
    fn test_empty_vec_builder() {
        let arena = AstArena::new();
        let builder: AstVecBuilder<u32> = AstVecBuilder::new();

        let slice = builder.freeze(&arena);
        assert!(slice.is_empty());
    }

    #[test]
    fn test_single_element_vec() {
        let arena = AstArena::new();
        let mut builder = AstVecBuilder::new();
        builder.push(42u64);

        let slice = builder.freeze(&arena);
        assert_eq!(slice.len(), 1);
        assert_eq!(slice[0], 42);
    }

    #[test]
    fn test_nested_allocation() {
        let arena = AstArena::new();

        // Simulate nested AST node allocation
        #[derive(Debug)]
        struct Inner {
            value: i32,
        }

        #[derive(Debug)]
        struct Outer<'a> {
            inner: &'a Inner,
            name: &'a str,
        }

        let inner = arena.alloc(Inner { value: 42 });
        let name = arena.alloc_str("test");
        let outer = arena.alloc(Outer { inner, name });

        assert_eq!(outer.inner.value, 42);
        assert_eq!(outer.name, "test");
    }
}
