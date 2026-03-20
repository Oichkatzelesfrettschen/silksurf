//! Bump arena allocator with generation tracking
//!
//! Key design decisions:
//! - Uses bumpalo for fast bump allocation
//! - Wraps with generation counter for safe reset
//! - All allocations tied to arena lifetime ('arena)
//! - Reset invalidates all references (compile-time safety via lifetimes)

use std::cell::Cell;

use bumpalo::Bump;

/// Generation counter for tracking arena resets
pub type Generation = u64;

/// Arena allocator with generation tracking
///
/// All allocations from this arena share lifetime 'arena.
/// When `reset()` is called, generation increments and all
/// previous allocations become invalid (enforced by lifetimes).
pub struct Arena {
    /// Underlying bump allocator
    bump: Bump,
    /// Current generation (increments on reset)
    generation: Cell<Generation>,
    /// Bytes allocated in current generation
    bytes_allocated: Cell<usize>,
}

impl Arena {
    /// Create a new arena with default capacity (1MB)
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(1024 * 1024)
    }

    /// Create arena with specified initial capacity
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            bump: Bump::with_capacity(capacity),
            generation: Cell::new(0),
            bytes_allocated: Cell::new(0),
        }
    }

    /// Allocate a value in the arena
    ///
    /// Returns a reference with lifetime tied to the arena.
    /// This reference is valid until `reset()` is called.
    #[inline]
    pub fn alloc<T>(&self, val: T) -> &T {
        let size = std::mem::size_of::<T>();
        self.bytes_allocated.set(self.bytes_allocated.get() + size);
        self.bump.alloc(val)
    }

    /// Allocate a slice in the arena
    #[inline]
    pub fn alloc_slice<T: Copy>(&self, slice: &[T]) -> &[T] {
        let size = std::mem::size_of_val(slice);
        self.bytes_allocated.set(self.bytes_allocated.get() + size);
        self.bump.alloc_slice_copy(slice)
    }

    /// Allocate a slice from an exact-size iterator.
    #[inline]
    pub fn alloc_slice_fill_iter<T, I>(&self, iter: I) -> &[T]
    where
        I: IntoIterator<Item = T>,
        I::IntoIter: ExactSizeIterator,
    {
        let iter = iter.into_iter();
        let len = iter.len();
        let size = std::mem::size_of::<T>() * len;
        self.bytes_allocated.set(self.bytes_allocated.get() + size);
        self.bump.alloc_slice_fill_iter(iter)
    }

    /// Allocate a string in the arena (interned copy)
    #[inline]
    pub fn alloc_str(&self, s: &str) -> &str {
        self.bytes_allocated
            .set(self.bytes_allocated.get() + s.len());
        self.bump.alloc_str(s)
    }

    /// Reset the arena, freeing all allocations
    ///
    /// After reset:
    /// - All previous allocations are invalid
    /// - Generation counter increments
    /// - Memory is reused for future allocations
    ///
    /// # Safety
    /// Caller must ensure no references to arena-allocated
    /// data are held across the reset boundary.
    pub fn reset(&mut self) {
        self.bump.reset();
        self.generation.set(self.generation.get() + 1);
        self.bytes_allocated.set(0);
    }

    /// Current generation number
    #[inline]
    #[must_use]
    pub fn generation(&self) -> Generation {
        self.generation.get()
    }

    /// Bytes allocated in current generation
    #[inline]
    #[must_use]
    pub fn bytes_allocated(&self) -> usize {
        self.bytes_allocated.get()
    }

    /// Total bytes allocated (including slack)
    #[inline]
    #[must_use]
    pub fn allocated_bytes_including_metadata(&self) -> usize {
        self.bump.allocated_bytes()
    }
}

impl Default for Arena {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alloc_and_reset() {
        let mut arena = Arena::new();
        assert_eq!(arena.generation(), 0);

        // Allocate some values
        let x = arena.alloc(42u64);
        let y = arena.alloc(123u64);
        assert_eq!(*x, 42);
        assert_eq!(*y, 123);
        assert!(arena.bytes_allocated() >= 16);

        // Reset increments generation
        arena.reset();
        assert_eq!(arena.generation(), 1);
        assert_eq!(arena.bytes_allocated(), 0);

        // Can allocate again
        let z = arena.alloc(999u64);
        assert_eq!(*z, 999);
    }

    #[test]
    fn test_alloc_str() {
        let arena = Arena::new();
        let s = arena.alloc_str("hello world");
        assert_eq!(s, "hello world");
        assert!(arena.bytes_allocated() >= 11);
    }

    #[test]
    fn test_alloc_slice() {
        let arena = Arena::new();
        let data = [1u32, 2, 3, 4, 5];
        let slice = arena.alloc_slice(&data);
        assert_eq!(slice, &[1, 2, 3, 4, 5]);
    }
}
