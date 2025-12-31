//! Inline Caching (IC) for property access optimization
//!
//! Inline caches store shape + slot offset pairs at property access sites.
//! When the object's shape matches the cached shape, we skip the lookup
//! and access the slot directly.
//!
//! IC states:
//! - Uninitialized: No cached entry
//! - Monomorphic: One shape cached (fastest path)
//! - Polymorphic: 2-4 shapes cached (still fast)
//! - Megamorphic: Too many shapes, fall back to generic lookup
//!
//! Design informed by V8's IC system and SpiderMonkey's CacheIR.

use crate::vm::shape::{PropertyKey, ShapeId};

/// Maximum entries in polymorphic IC before going megamorphic
const MAX_POLYMORPHIC_ENTRIES: usize = 4;

/// A single inline cache entry
#[derive(Debug, Clone, Copy)]
pub struct ICEntry {
    /// Shape ID this entry is valid for
    pub shape_id: ShapeId,
    /// Cached slot offset in object storage
    pub slot: u32,
}

/// Inline cache state
#[derive(Debug, Clone)]
pub enum ICState {
    /// No cached entries yet
    Uninitialized,
    /// Single shape cached (most common case)
    Monomorphic(ICEntry),
    /// Multiple shapes (2-4)
    Polymorphic(Vec<ICEntry>),
    /// Too many shapes - use generic lookup
    Megamorphic,
}

impl Default for ICState {
    fn default() -> Self {
        Self::Uninitialized
    }
}

/// Inline cache for a single property access site
#[derive(Debug, Clone)]
pub struct InlineCache {
    /// Property key being accessed
    pub key: PropertyKey,
    /// Current cache state
    pub state: ICState,
    /// Hit count for profiling
    pub hits: u32,
    /// Miss count for profiling
    pub misses: u32,
}

impl InlineCache {
    /// Create new uninitialized cache for a property key
    pub fn new(key: PropertyKey) -> Self {
        Self {
            key,
            state: ICState::Uninitialized,
            hits: 0,
            misses: 0,
        }
    }

    /// Try to find a cached slot for the given shape
    ///
    /// Returns Some(slot) on cache hit, None on miss.
    #[inline]
    pub fn lookup(&mut self, shape_id: ShapeId) -> Option<u32> {
        match &self.state {
            ICState::Uninitialized => {
                self.misses += 1;
                None
            }
            ICState::Monomorphic(entry) => {
                if entry.shape_id == shape_id {
                    self.hits += 1;
                    Some(entry.slot)
                } else {
                    self.misses += 1;
                    None
                }
            }
            ICState::Polymorphic(entries) => {
                for entry in entries {
                    if entry.shape_id == shape_id {
                        self.hits += 1;
                        return Some(entry.slot);
                    }
                }
                self.misses += 1;
                None
            }
            ICState::Megamorphic => {
                self.misses += 1;
                None
            }
        }
    }

    /// Update cache with a new shape -> slot mapping
    pub fn update(&mut self, shape_id: ShapeId, slot: u32) {
        let entry = ICEntry { shape_id, slot };

        match &mut self.state {
            ICState::Uninitialized => {
                self.state = ICState::Monomorphic(entry);
            }
            ICState::Monomorphic(existing) => {
                if existing.shape_id == shape_id {
                    // Same shape, update slot (shouldn't change normally)
                    existing.slot = slot;
                } else {
                    // Different shape - transition to polymorphic
                    self.state = ICState::Polymorphic(vec![*existing, entry]);
                }
            }
            ICState::Polymorphic(entries) => {
                // Check if we already have this shape
                for e in entries.iter_mut() {
                    if e.shape_id == shape_id {
                        e.slot = slot;
                        return;
                    }
                }
                // New shape
                if entries.len() < MAX_POLYMORPHIC_ENTRIES {
                    entries.push(entry);
                } else {
                    // Too many shapes - go megamorphic
                    self.state = ICState::Megamorphic;
                }
            }
            ICState::Megamorphic => {
                // Already megamorphic, no change
            }
        }
    }

    /// Reset cache to uninitialized state
    pub fn reset(&mut self) {
        self.state = ICState::Uninitialized;
        self.hits = 0;
        self.misses = 0;
    }

    /// Get hit rate (0.0 to 1.0)
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }

    /// Check if cache is megamorphic
    pub fn is_megamorphic(&self) -> bool {
        matches!(self.state, ICState::Megamorphic)
    }
}

/// Inline cache vector for all property access sites in a function
#[derive(Debug, Default)]
pub struct ICVector {
    /// Caches indexed by IC ID (bytecode offset or dedicated index)
    caches: Vec<InlineCache>,
}

impl ICVector {
    /// Create new empty IC vector
    pub fn new() -> Self {
        Self { caches: Vec::new() }
    }

    /// Create IC vector with capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            caches: Vec::with_capacity(capacity),
        }
    }

    /// Add a new cache for a property access, returning its ID
    pub fn add(&mut self, key: PropertyKey) -> u32 {
        let id = self.caches.len() as u32;
        self.caches.push(InlineCache::new(key));
        id
    }

    /// Get cache by ID
    #[inline]
    pub fn get(&self, id: u32) -> Option<&InlineCache> {
        self.caches.get(id as usize)
    }

    /// Get mutable cache by ID
    #[inline]
    pub fn get_mut(&mut self, id: u32) -> Option<&mut InlineCache> {
        self.caches.get_mut(id as usize)
    }

    /// Number of caches
    pub fn len(&self) -> usize {
        self.caches.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.caches.is_empty()
    }

    /// Reset all caches (e.g., after GC that might invalidate shapes)
    pub fn reset_all(&mut self) {
        for cache in &mut self.caches {
            cache.reset();
        }
    }

    /// Get aggregate statistics
    pub fn stats(&self) -> ICStats {
        let mut stats = ICStats::default();
        for cache in &self.caches {
            stats.total += 1;
            stats.hits += cache.hits;
            stats.misses += cache.misses;
            match cache.state {
                ICState::Uninitialized => stats.uninitialized += 1,
                ICState::Monomorphic(_) => stats.monomorphic += 1,
                ICState::Polymorphic(_) => stats.polymorphic += 1,
                ICState::Megamorphic => stats.megamorphic += 1,
            }
        }
        stats
    }
}

/// Aggregate IC statistics
#[derive(Debug, Default)]
pub struct ICStats {
    pub total: u32,
    pub uninitialized: u32,
    pub monomorphic: u32,
    pub polymorphic: u32,
    pub megamorphic: u32,
    pub hits: u32,
    pub misses: u32,
}

impl ICStats {
    /// Overall hit rate
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

/// Load IC - caches property loads (GetProp/GetElem)
#[derive(Debug)]
pub struct LoadIC {
    /// Property key
    pub key: PropertyKey,
    /// Cached entries
    entries: Vec<LoadICEntry>,
    /// Hit/miss stats
    hits: u32,
    misses: u32,
}

/// Load IC entry with getter support
#[derive(Debug, Clone)]
pub struct LoadICEntry {
    /// Shape ID
    pub shape_id: ShapeId,
    /// Slot offset (for data properties)
    pub slot: Option<u32>,
    /// Whether this is a getter (requires function call)
    pub is_getter: bool,
}

impl LoadIC {
    pub fn new(key: PropertyKey) -> Self {
        Self {
            key,
            entries: Vec::with_capacity(4),
            hits: 0,
            misses: 0,
        }
    }

    /// Fast path lookup
    #[inline]
    pub fn lookup(&mut self, shape_id: ShapeId) -> Option<&LoadICEntry> {
        for entry in &self.entries {
            if entry.shape_id == shape_id {
                self.hits += 1;
                return Some(entry);
            }
        }
        self.misses += 1;
        None
    }

    /// Add entry for data property
    pub fn add_data(&mut self, shape_id: ShapeId, slot: u32) {
        if self.entries.len() < MAX_POLYMORPHIC_ENTRIES {
            self.entries.push(LoadICEntry {
                shape_id,
                slot: Some(slot),
                is_getter: false,
            });
        }
    }

    /// Add entry for getter property
    pub fn add_getter(&mut self, shape_id: ShapeId) {
        if self.entries.len() < MAX_POLYMORPHIC_ENTRIES {
            self.entries.push(LoadICEntry {
                shape_id,
                slot: None,
                is_getter: true,
            });
        }
    }
}

/// Store IC - caches property stores (SetProp/SetElem)
#[derive(Debug)]
pub struct StoreIC {
    /// Property key
    pub key: PropertyKey,
    /// Cached entries
    entries: Vec<StoreICEntry>,
    /// Hit/miss stats
    hits: u32,
    misses: u32,
}

/// Store IC entry with transition support
#[derive(Debug, Clone, Copy)]
pub struct StoreICEntry {
    /// Shape ID before store
    pub from_shape: ShapeId,
    /// Shape ID after store (may be same if property exists)
    pub to_shape: ShapeId,
    /// Slot offset
    pub slot: u32,
    /// Whether this is a new property (transition)
    pub is_transition: bool,
}

impl StoreIC {
    pub fn new(key: PropertyKey) -> Self {
        Self {
            key,
            entries: Vec::with_capacity(4),
            hits: 0,
            misses: 0,
        }
    }

    /// Fast path lookup
    #[inline]
    pub fn lookup(&mut self, shape_id: ShapeId) -> Option<&StoreICEntry> {
        for entry in &self.entries {
            if entry.from_shape == shape_id {
                self.hits += 1;
                return Some(entry);
            }
        }
        self.misses += 1;
        None
    }

    /// Add entry for existing property update
    pub fn add_update(&mut self, shape_id: ShapeId, slot: u32) {
        if self.entries.len() < MAX_POLYMORPHIC_ENTRIES {
            self.entries.push(StoreICEntry {
                from_shape: shape_id,
                to_shape: shape_id,
                slot,
                is_transition: false,
            });
        }
    }

    /// Add entry for new property (shape transition)
    pub fn add_transition(&mut self, from_shape: ShapeId, to_shape: ShapeId, slot: u32) {
        if self.entries.len() < MAX_POLYMORPHIC_ENTRIES {
            self.entries.push(StoreICEntry {
                from_shape,
                to_shape,
                slot,
                is_transition: true,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ic_uninitialized() {
        let key = PropertyKey::String(1);
        let mut ic = InlineCache::new(key);

        assert!(ic.lookup(0).is_none());
        assert_eq!(ic.misses, 1);
        assert!(!ic.is_megamorphic());
    }

    #[test]
    fn test_ic_monomorphic() {
        let key = PropertyKey::String(1);
        let mut ic = InlineCache::new(key);

        // Update with shape 0, slot 5
        ic.update(0, 5);

        // Lookup should hit
        assert_eq!(ic.lookup(0), Some(5));
        assert_eq!(ic.hits, 1);

        // Different shape should miss
        assert_eq!(ic.lookup(1), None);
        assert_eq!(ic.misses, 1);
    }

    #[test]
    fn test_ic_polymorphic() {
        let key = PropertyKey::String(1);
        let mut ic = InlineCache::new(key);

        // Add multiple shapes
        ic.update(0, 5);
        ic.update(1, 10);
        ic.update(2, 15);

        // All should hit
        assert_eq!(ic.lookup(0), Some(5));
        assert_eq!(ic.lookup(1), Some(10));
        assert_eq!(ic.lookup(2), Some(15));
        assert_eq!(ic.hits, 3);
    }

    #[test]
    fn test_ic_megamorphic() {
        let key = PropertyKey::String(1);
        let mut ic = InlineCache::new(key);

        // Add more than MAX_POLYMORPHIC_ENTRIES shapes
        for i in 0..=MAX_POLYMORPHIC_ENTRIES as u32 {
            ic.update(i, i * 10);
        }

        assert!(ic.is_megamorphic());

        // All lookups return None in megamorphic state
        assert_eq!(ic.lookup(0), None);
    }

    #[test]
    fn test_ic_vector() {
        let mut vec = ICVector::new();

        let key1 = PropertyKey::String(1);
        let key2 = PropertyKey::String(2);

        let id1 = vec.add(key1);
        let id2 = vec.add(key2);

        assert_eq!(id1, 0);
        assert_eq!(id2, 1);
        assert_eq!(vec.len(), 2);

        // Update and lookup
        vec.get_mut(id1).unwrap().update(0, 5);
        assert_eq!(vec.get_mut(id1).unwrap().lookup(0), Some(5));
    }

    #[test]
    fn test_ic_stats() {
        let mut vec = ICVector::new();

        let key = PropertyKey::String(1);
        let id = vec.add(key);

        let ic = vec.get_mut(id).unwrap();
        ic.update(0, 5);
        ic.lookup(0); // hit
        ic.lookup(0); // hit
        ic.lookup(1); // miss

        let stats = vec.stats();
        assert_eq!(stats.total, 1);
        assert_eq!(stats.monomorphic, 1);
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.misses, 1);
        assert!((stats.hit_rate() - 0.666).abs() < 0.01);
    }

    #[test]
    fn test_load_ic() {
        let key = PropertyKey::String(1);
        let mut ic = LoadIC::new(key);

        ic.add_data(0, 5);
        ic.add_data(1, 10);

        let entry = ic.lookup(0).unwrap();
        assert_eq!(entry.slot, Some(5));
        assert!(!entry.is_getter);

        let entry = ic.lookup(1).unwrap();
        assert_eq!(entry.slot, Some(10));
    }

    #[test]
    fn test_store_ic() {
        let key = PropertyKey::String(1);
        let mut ic = StoreIC::new(key);

        // Existing property update
        ic.add_update(0, 5);

        // New property transition
        ic.add_transition(1, 2, 0);

        let entry = ic.lookup(0).unwrap();
        assert_eq!(entry.slot, 5);
        assert!(!entry.is_transition);

        let entry = ic.lookup(1).unwrap();
        assert_eq!(entry.from_shape, 1);
        assert_eq!(entry.to_shape, 2);
        assert!(entry.is_transition);
    }

    #[test]
    fn test_ic_hit_rate() {
        let key = PropertyKey::String(1);
        let mut ic = InlineCache::new(key);

        ic.update(0, 5);

        // 3 hits, 1 miss
        ic.lookup(0);
        ic.lookup(0);
        ic.lookup(0);
        ic.lookup(1);

        assert!((ic.hit_rate() - 0.75).abs() < 0.01);
    }
}
