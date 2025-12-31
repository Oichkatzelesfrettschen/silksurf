//! Generational indices for safe arena references
//!
//! Solves the ABA problem: when an arena slot is reused,
//! the generation counter ensures old references are invalid.
//!
//! Inspired by `generational-arena` crate and `RustConf` 2018 ECS talk.

use crate::gc::arena::Generation;

/// A generational index into an arena
///
/// Contains both the slot index and the generation when
/// the value was allocated. If the arena's generation
/// doesn't match, the reference is stale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GenerationalIndex {
    /// Slot index in the arena
    index: u32,
    /// Generation when this slot was allocated
    generation: Generation,
}

impl GenerationalIndex {
    /// Create a new generational index
    #[inline]
    #[must_use]
    pub const fn new(index: u32, generation: Generation) -> Self {
        Self { index, generation }
    }

    /// Get the slot index
    #[inline]
    #[must_use]
    pub const fn index(self) -> u32 {
        self.index
    }

    /// Get the generation
    #[inline]
    #[must_use]
    pub const fn generation(self) -> Generation {
        self.generation
    }

    /// Check if this index is valid for the given generation
    #[inline]
    #[must_use]
    pub const fn is_valid(self, current_generation: Generation) -> bool {
        self.generation == current_generation
    }
}

/// A slot in a generational arena
#[derive(Debug)]
pub struct Slot<T> {
    /// The value (if occupied)
    value: Option<T>,
    /// Generation when this slot was last written
    generation: Generation,
}

impl<T> Slot<T> {
    /// Create an empty slot
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            value: None,
            generation: 0,
        }
    }

    /// Insert a value, returning its generational index
    pub fn insert(&mut self, value: T, generation: Generation) -> Option<T> {
        self.generation = generation;
        self.value.replace(value)
    }

    /// Get the value if the generation matches
    #[must_use]
    pub fn get(&self, expected_gen: Generation) -> Option<&T> {
        if self.generation == expected_gen {
            self.value.as_ref()
        } else {
            None
        }
    }

    /// Get mutable value if the generation matches
    pub fn get_mut(&mut self, expected_gen: Generation) -> Option<&mut T> {
        if self.generation == expected_gen {
            self.value.as_mut()
        } else {
            None
        }
    }

    /// Remove the value
    pub fn remove(&mut self) -> Option<T> {
        self.value.take()
    }

    /// Check if occupied
    #[must_use]
    pub const fn is_occupied(&self) -> bool {
        self.value.is_some()
    }
}

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Self::empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generational_index_validity() {
        let idx = GenerationalIndex::new(5, 10);
        assert!(idx.is_valid(10));
        assert!(!idx.is_valid(11));
        assert!(!idx.is_valid(9));
    }

    #[test]
    fn test_slot_generation_check() {
        let mut slot = Slot::empty();
        slot.insert(42, 5);

        assert_eq!(slot.get(5), Some(&42));
        assert_eq!(slot.get(6), None); // Wrong generation
        assert_eq!(slot.get(4), None); // Wrong generation
    }
}
