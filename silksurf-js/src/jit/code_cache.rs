//! Code cache for JIT-compiled functions
//!
//! Manages compiled native code with LRU eviction.

use std::collections::HashMap;
use super::compiler::{CompiledFunction, JitError};
use super::MAX_CACHED_FUNCTIONS;

/// Cache for compiled native functions
pub struct CodeCache {
    /// Map from chunk index to compiled function
    functions: HashMap<usize, CompiledFunction>,
    /// Access order for LRU eviction (most recent at end)
    access_order: Vec<usize>,
    /// Total code size in bytes
    total_size: usize,
}

impl CodeCache {
    /// Create a new empty code cache
    pub fn new() -> Self {
        Self {
            functions: HashMap::new(),
            access_order: Vec::new(),
            total_size: 0,
        }
    }

    /// Get a compiled function by chunk index
    pub fn get(&self, chunk_idx: usize) -> Option<&CompiledFunction> {
        self.functions.get(&chunk_idx)
    }

    /// Get a compiled function and update access order
    pub fn get_mut(&mut self, chunk_idx: usize) -> Option<&CompiledFunction> {
        if self.functions.contains_key(&chunk_idx) {
            // Update access order (move to end)
            self.access_order.retain(|&x| x != chunk_idx);
            self.access_order.push(chunk_idx);
            self.functions.get(&chunk_idx)
        } else {
            None
        }
    }

    /// Insert a compiled function
    pub fn insert(&mut self, chunk_idx: usize, func: CompiledFunction) -> Result<(), JitError> {
        // Evict if at capacity
        while self.functions.len() >= MAX_CACHED_FUNCTIONS {
            self.evict_lru();
        }

        self.total_size += func.size;
        self.access_order.push(chunk_idx);
        self.functions.insert(chunk_idx, func);

        Ok(())
    }

    /// Remove least recently used function
    fn evict_lru(&mut self) {
        if let Some(chunk_idx) = self.access_order.first().copied() {
            self.access_order.remove(0);
            if let Some(func) = self.functions.remove(&chunk_idx) {
                self.total_size = self.total_size.saturating_sub(func.size);
            }
        }
    }

    /// Remove a specific function
    pub fn remove(&mut self, chunk_idx: usize) -> Option<CompiledFunction> {
        self.access_order.retain(|&x| x != chunk_idx);
        if let Some(func) = self.functions.remove(&chunk_idx) {
            self.total_size = self.total_size.saturating_sub(func.size);
            Some(func)
        } else {
            None
        }
    }

    /// Clear all cached functions
    pub fn clear(&mut self) {
        self.functions.clear();
        self.access_order.clear();
        self.total_size = 0;
    }

    /// Number of cached functions
    pub fn len(&self) -> usize {
        self.functions.len()
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty()
    }

    /// Total size of cached code
    pub fn total_size(&self) -> usize {
        self.total_size
    }

    /// Check if a chunk is cached
    pub fn contains(&self, chunk_idx: usize) -> bool {
        self.functions.contains_key(&chunk_idx)
    }
}

impl Default for CodeCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_func(chunk_idx: usize, size: usize) -> CompiledFunction {
        CompiledFunction {
            ptr: std::ptr::null(),
            size,
            chunk_idx,
        }
    }

    #[test]
    fn test_cache_insert_get() {
        let mut cache = CodeCache::new();
        cache.insert(0, make_func(0, 100)).unwrap();
        cache.insert(1, make_func(1, 200)).unwrap();

        assert!(cache.contains(0));
        assert!(cache.contains(1));
        assert!(!cache.contains(2));

        assert_eq!(cache.len(), 2);
        assert_eq!(cache.total_size(), 300);
    }

    #[test]
    fn test_cache_remove() {
        let mut cache = CodeCache::new();
        cache.insert(0, make_func(0, 100)).unwrap();
        cache.insert(1, make_func(1, 200)).unwrap();

        let removed = cache.remove(0);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().chunk_idx, 0);

        assert!(!cache.contains(0));
        assert!(cache.contains(1));
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.total_size(), 200);
    }

    #[test]
    fn test_cache_clear() {
        let mut cache = CodeCache::new();
        cache.insert(0, make_func(0, 100)).unwrap();
        cache.insert(1, make_func(1, 200)).unwrap();

        cache.clear();

        assert!(cache.is_empty());
        assert_eq!(cache.total_size(), 0);
    }

    #[test]
    fn test_lru_eviction() {
        // Create a small cache for testing
        let mut cache = CodeCache::new();

        // Fill the cache (note: MAX_CACHED_FUNCTIONS is 1024, so this test is conceptual)
        for i in 0..5 {
            cache.insert(i, make_func(i, 10)).unwrap();
        }

        // Access item 0 to make it recently used
        cache.get_mut(0);

        // The access order should now have 0 at the end
        assert!(cache.access_order.last() == Some(&0));
    }
}
