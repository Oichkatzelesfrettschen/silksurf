//! Mark-Sweep Garbage Collector
//!
//! # Algorithm Overview
//!
//! We implement a non-moving, tri-color mark-sweep collector:
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    TRI-COLOR ABSTRACTION                    │
//! ├─────────────────────────────────────────────────────────────┤
//! │  WHITE: Potentially garbage (unmarked)                      │
//! │  GRAY:  Reachable, but children not yet scanned             │
//! │  BLACK: Reachable, all children scanned                     │
//! │                                                             │
//! │  Invariant: No BLACK object points directly to WHITE        │
//! │  (This enables incremental/concurrent collection later)     │
//! └─────────────────────────────────────────────────────────────┘
//!
//! ┌─────────────────────────────────────────────────────────────┐
//! │                      MARK PHASE                             │
//! ├─────────────────────────────────────────────────────────────┤
//! │  1. All objects start WHITE                                 │
//! │  2. Push roots onto worklist (color them GRAY)              │
//! │  3. While worklist non-empty:                               │
//! │     a. Pop object, scan its fields                          │
//! │     b. For each WHITE child, color GRAY and push            │
//! │     c. Color popped object BLACK                            │
//! │  4. All reachable objects are now BLACK                     │
//! └─────────────────────────────────────────────────────────────┘
//!
//! ┌─────────────────────────────────────────────────────────────┐
//! │                      SWEEP PHASE                            │
//! ├─────────────────────────────────────────────────────────────┤
//! │  1. Walk all allocated objects linearly                     │
//! │  2. WHITE objects -> add to free list                       │
//! │  3. BLACK objects -> reset to WHITE for next cycle          │
//! │  4. Coalesce adjacent free blocks                           │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Memory Layout
//!
//! ```text
//! ┌────────────────────────────────────────┐
//! │            OBJECT HEADER (16 bytes)    │
//! ├────────────────────────────────────────┤
//! │  mark_bits: u8   [color + flags]       │
//! │  type_tag:  u8   [object type]         │
//! │  padding:   u16                        │
//! │  size:      u32  [total size in bytes] │
//! │  next_free: u64  [free list link]      │
//! ├────────────────────────────────────────┤
//! │            OBJECT PAYLOAD              │
//! │  (size - 16 bytes)                     │
//! └────────────────────────────────────────┘
//! ```

use std::alloc::{Layout, alloc, dealloc};
use std::collections::HashMap;
use std::ptr::NonNull;

use bitvec::vec::BitVec;
use static_assertions::const_assert_eq;

// ============================================================================
// Constants
// ============================================================================

/// Object header size in bytes
pub const HEADER_SIZE: usize = 16;

/// Minimum allocation size (header + minimum payload)
pub const MIN_ALLOC_SIZE: usize = 32;

/// Alignment for all allocations
pub const ALIGNMENT: usize = 8;

/// Size classes for segregated free lists (powers of 2)
pub const SIZE_CLASSES: [usize; 8] = [32, 64, 128, 256, 512, 1024, 2048, 4096];

/// Large object threshold (objects >= this go to large object space)
pub const LARGE_OBJECT_THRESHOLD: usize = 4096;

// ============================================================================
// Tri-Color Marking
// ============================================================================

/// GC color for tri-color marking
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Color {
    /// Potentially garbage (not yet visited)
    White = 0,
    /// Reachable, children not yet scanned
    Gray = 1,
    /// Reachable, fully scanned
    Black = 2,
}

/// Object type tags for runtime type identification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TypeTag {
    /// Free block (not allocated)
    Free = 0,
    /// JavaScript object with shape
    Object = 1,
    /// JavaScript array
    Array = 2,
    /// JavaScript function/closure
    Function = 3,
    /// JavaScript string (heap-allocated)
    String = 4,
    /// Symbol
    Symbol = 5,
    /// Buffer backing store
    Buffer = 6,
    /// Weak reference target slot
    WeakRef = 7,
}

// ============================================================================
// Object Header
// ============================================================================

/// Header prepended to every GC-managed object
///
/// Layout optimized for cache-line alignment and fast field access.
#[derive(Debug)]
#[repr(C)]
pub struct GcHeader {
    /// Mark bits: lower 2 bits = color, upper 6 bits = flags
    mark_bits: u8,
    /// Object type tag
    type_tag: u8,
    /// Reserved for future use
    _padding: u16,
    /// Total size including header (aligned to ALIGNMENT)
    size: u32,
    /// Next pointer for free list (only valid when Free)
    next_free: u64,
}

// Compile-time verification that GcHeader is exactly HEADER_SIZE bytes
const_assert_eq!(std::mem::size_of::<GcHeader>(), HEADER_SIZE);

impl GcHeader {
    /// Create header for a new allocation
    #[inline]
    #[must_use]
    pub const fn new(type_tag: TypeTag, size: u32) -> Self {
        Self {
            mark_bits: Color::White as u8,
            type_tag: type_tag as u8,
            _padding: 0,
            size,
            next_free: 0,
        }
    }

    /// Get current GC color
    #[inline]
    #[must_use]
    pub fn color(&self) -> Color {
        match self.mark_bits & 0b11 {
            1 => Color::Gray,
            2 => Color::Black,
            _ => Color::White, // 0 or invalid -> White
        }
    }

    /// Set GC color
    #[inline]
    pub fn set_color(&mut self, color: Color) {
        self.mark_bits = (self.mark_bits & !0b11) | (color as u8);
    }

    /// Get object type
    #[inline]
    #[must_use]
    pub fn type_tag(&self) -> TypeTag {
        match self.type_tag {
            1 => TypeTag::Object,
            2 => TypeTag::Array,
            3 => TypeTag::Function,
            4 => TypeTag::String,
            5 => TypeTag::Symbol,
            6 => TypeTag::Buffer,
            7 => TypeTag::WeakRef,
            _ => TypeTag::Free, // 0 or invalid -> Free
        }
    }

    /// Check if this is a free block
    #[inline]
    #[must_use]
    pub fn is_free(&self) -> bool {
        self.type_tag == TypeTag::Free as u8
    }

    /// Get total size (header + payload)
    #[inline]
    #[must_use]
    pub fn size(&self) -> usize {
        self.size as usize
    }

    /// Get payload size (excluding header)
    #[inline]
    #[must_use]
    pub fn payload_size(&self) -> usize {
        self.size as usize - HEADER_SIZE
    }

    /// Get pointer to payload
    #[inline]
    #[must_use]
    pub fn payload_ptr(&self) -> *mut u8 {
        // SAFETY: We're computing an offset within the same allocation
        unsafe {
            std::ptr::from_ref(self)
                .cast::<u8>()
                .cast_mut()
                .add(HEADER_SIZE)
        }
    }
}

// ============================================================================
// GC Handle (Safe Reference)
// ============================================================================

/// Type-safe handle to a GC-managed object
#[derive(Debug, Clone, Copy)]
pub struct GcRef {
    /// Pointer to object header
    ptr: NonNull<GcHeader>,
}

impl GcRef {
    /// Create from raw pointer (unsafe - caller must ensure validity)
    ///
    /// # Safety
    /// Pointer must point to a valid `GcHeader`
    #[inline]
    #[must_use]
    pub unsafe fn from_raw(ptr: *mut GcHeader) -> Option<Self> {
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    /// Get header reference
    #[inline]
    #[must_use]
    pub fn header(&self) -> &GcHeader {
        unsafe { self.ptr.as_ref() }
    }

    /// Get mutable header reference
    #[inline]
    pub fn header_mut(&mut self) -> &mut GcHeader {
        unsafe { self.ptr.as_mut() }
    }

    /// Get raw pointer
    #[inline]
    #[must_use]
    pub fn as_ptr(&self) -> *mut GcHeader {
        self.ptr.as_ptr()
    }

    /// Get payload as typed reference
    ///
    /// # Safety
    /// Caller must ensure T matches the actual payload type
    #[inline]
    #[must_use]
    pub unsafe fn payload<T>(&self) -> &T {
        unsafe { &*self.header().payload_ptr().cast::<T>() }
    }

    /// Get payload as mutable typed reference
    ///
    /// # Safety
    /// Caller must ensure T matches the actual payload type
    #[inline]
    pub unsafe fn payload_mut<T>(&mut self) -> &mut T {
        unsafe { &mut *self.header().payload_ptr().cast::<T>() }
    }
}

impl PartialEq for GcRef {
    fn eq(&self, other: &Self) -> bool {
        self.ptr == other.ptr
    }
}

impl Eq for GcRef {}

impl std::hash::Hash for GcRef {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.ptr.as_ptr().hash(state);
    }
}

// ============================================================================
// Free List
// ============================================================================

/// Free list for a specific size class
#[derive(Debug)]
pub struct FreeList {
    /// Head of the free list
    head: Option<NonNull<GcHeader>>,
    /// Number of blocks in list
    count: usize,
    /// Size class this list manages
    size_class: usize,
}

impl FreeList {
    /// Create empty free list for given size class
    #[must_use]
    pub const fn new(size_class: usize) -> Self {
        Self {
            head: None,
            count: 0,
            size_class,
        }
    }

    /// Push a block onto the free list
    ///
    /// # Safety
    /// Block must be properly aligned and sized for this size class
    pub unsafe fn push(&mut self, block: NonNull<GcHeader>) {
        let header = block.as_ptr();
        unsafe {
            (*header).type_tag = TypeTag::Free as u8;
            (*header).next_free = self.head.map_or(0, |p| p.as_ptr() as u64);
        }
        self.head = Some(block);
        self.count += 1;
    }

    /// Pop a block from the free list
    pub fn pop(&mut self) -> Option<NonNull<GcHeader>> {
        let block = self.head?;
        unsafe {
            let next = (*block.as_ptr()).next_free;
            self.head = if next == 0 {
                None
            } else {
                NonNull::new(next as *mut GcHeader)
            };
        }
        self.count -= 1;
        Some(block)
    }

    /// Check if empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.head.is_none()
    }

    /// Number of available blocks
    #[must_use]
    pub fn len(&self) -> usize {
        self.count
    }
}

// ============================================================================
// Heap Statistics
// ============================================================================

/// GC statistics for monitoring and tuning
#[derive(Debug, Default, Clone)]
pub struct GcStats {
    /// Total bytes allocated
    pub bytes_allocated: usize,
    /// Total bytes in use (after last GC)
    pub bytes_in_use: usize,
    /// Number of objects allocated
    pub objects_allocated: u64,
    /// Number of objects collected
    pub objects_collected: u64,
    /// Number of GC cycles run
    pub gc_cycles: u64,
    /// Total time spent in GC (nanoseconds)
    pub gc_time_ns: u64,
    /// Peak memory usage
    pub peak_bytes: usize,
}

impl GcStats {
    /// Calculate collection efficiency (bytes freed per second)
    ///
    /// Returns bytes freed divided by GC time in seconds.
    /// Precision loss in conversion is acceptable for monitoring purposes.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn efficiency(&self) -> f64 {
        if self.gc_time_ns == 0 {
            0.0
        } else {
            let bytes_freed = self.bytes_allocated.saturating_sub(self.bytes_in_use);
            bytes_freed as f64 / (self.gc_time_ns as f64 / 1_000_000_000.0)
        }
    }
}

// ============================================================================
// Managed Heap
// ============================================================================

/// The garbage-collected heap
#[derive(Debug)]
pub struct Heap {
    /// Segregated free lists by size class
    free_lists: [FreeList; 8],
    /// Large object list (objects >= `LARGE_OBJECT_THRESHOLD`)
    large_objects: Vec<NonNull<GcHeader>>,
    /// All allocated blocks (for sweep traversal)
    all_blocks: Vec<NonNull<GcHeader>>,
    /// Mark bitmap for current GC cycle (parallel to `all_blocks`)
    mark_bits: BitVec,
    /// Pointer-to-index map for mark bitmap
    mark_index: HashMap<*mut GcHeader, usize>,
    /// Gray worklist for marking
    gray_worklist: Vec<GcRef>,
    /// Statistics
    stats: GcStats,
    /// Allocation threshold to trigger GC
    gc_threshold: usize,
}

impl Heap {
    /// Create a new heap with default settings
    #[must_use]
    pub fn new() -> Self {
        Self {
            free_lists: [
                FreeList::new(SIZE_CLASSES[0]),
                FreeList::new(SIZE_CLASSES[1]),
                FreeList::new(SIZE_CLASSES[2]),
                FreeList::new(SIZE_CLASSES[3]),
                FreeList::new(SIZE_CLASSES[4]),
                FreeList::new(SIZE_CLASSES[5]),
                FreeList::new(SIZE_CLASSES[6]),
                FreeList::new(SIZE_CLASSES[7]),
            ],
            large_objects: Vec::new(),
            all_blocks: Vec::new(),
            mark_bits: BitVec::new(),
            mark_index: HashMap::new(),
            gray_worklist: Vec::with_capacity(256),
            stats: GcStats::default(),
            gc_threshold: 1024 * 1024, // 1 MB initial threshold
        }
    }

    /// Find size class index for given size
    fn size_class_index(size: usize) -> Option<usize> {
        SIZE_CLASSES.iter().position(|&sc| sc >= size)
    }

    /// Allocate memory for an object
    ///
    /// Returns None if allocation fails.
    pub fn alloc(&mut self, type_tag: TypeTag, payload_size: usize) -> Option<GcRef> {
        let total_size = (HEADER_SIZE + payload_size).next_multiple_of(ALIGNMENT);
        let total_size = total_size.max(MIN_ALLOC_SIZE);

        // Try free list first
        let block = if total_size < LARGE_OBJECT_THRESHOLD {
            Self::size_class_index(total_size).and_then(|idx| self.free_lists[idx].pop())
        } else {
            None
        };

        // Allocate from system if no free block available
        #[allow(clippy::cast_ptr_alignment)] // Allocation is properly aligned via Layout
        let block = if let Some(b) = block {
            b
        } else {
            let alloc_size = if total_size < LARGE_OBJECT_THRESHOLD {
                Self::size_class_index(total_size).map_or(total_size, |i| SIZE_CLASSES[i])
            } else {
                total_size
            };

            let layout = Layout::from_size_align(alloc_size, ALIGNMENT).ok()?;
            let ptr = unsafe { alloc(layout) };
            let block = NonNull::new(ptr.cast::<GcHeader>())?;

            self.all_blocks.push(block);
            self.mark_bits.push(false);
            self.stats.bytes_allocated += alloc_size;
            self.stats.peak_bytes = self.stats.peak_bytes.max(self.stats.bytes_allocated);

            if alloc_size >= LARGE_OBJECT_THRESHOLD {
                self.large_objects.push(block);
            }

            block
        };

        // Initialize header
        #[allow(clippy::cast_possible_truncation)] // Size is bounded by alloc_size <= 4GB
        unsafe {
            let header = block.as_ptr();
            *header = GcHeader::new(type_tag, total_size as u32);
        }

        self.stats.objects_allocated += 1;

        unsafe { GcRef::from_raw(block.as_ptr()) }
    }

    /// Mark an object as reachable (add to gray worklist)
    #[inline]
    pub fn mark(&mut self, obj: GcRef) {
        if let Some(&idx) = self.mark_index.get(&obj.as_ptr()) {
            debug_assert!(idx < self.mark_bits.len());
            if !self.mark_bits[idx] {
                self.mark_bits.set(idx, true);
                let header = unsafe { &mut *obj.as_ptr() };
                header.set_color(Color::Gray);
                self.gray_worklist.push(obj);
            }
            return;
        }

        let header = unsafe { &mut *obj.as_ptr() };
        if header.color() == Color::White {
            header.set_color(Color::Gray);
            self.gray_worklist.push(obj);
        }
    }

    /// Process the gray worklist until empty
    ///
    /// The `trace_fn` callback is called for each object to enumerate its children.
    pub fn process_gray_worklist<F>(&mut self, mut trace_fn: F)
    where
        F: FnMut(GcRef, &mut Vec<GcRef>),
    {
        let mut children = Vec::new();

        while let Some(obj) = self.gray_worklist.pop() {
            children.clear();
            trace_fn(obj, &mut children);

            // Mark all children
            for child in children.drain(..) {
                self.mark(child);
            }

            // Object fully scanned, color it black
            unsafe {
                (*obj.as_ptr()).set_color(Color::Black);
            }
        }
    }

    /// Sweep phase: collect white objects, reset black to white
    ///
    /// # Panics
    /// Panics if a large object has an invalid size/alignment combination
    /// (should never happen with properly allocated objects).
    pub fn sweep(&mut self) {
        self.sweep_with_callback(|_| {});
    }

    /// Sweep phase with callback for each collected object
    ///
    /// The callback is invoked for each WHITE object before it is freed,
    /// allowing weak references to be cleared and finalizers to be queued.
    ///
    /// # Panics
    /// Panics if a large object has an invalid size/alignment combination
    /// (should never happen with properly allocated objects).
    pub fn sweep_with_callback<F>(&mut self, mut on_collect: F)
    where
        F: FnMut(GcRef),
    {
        let mut retained = Vec::with_capacity(self.all_blocks.len());
        let mut bytes_freed = 0usize;
        let mut objects_freed = 0u64;

        let use_bitmap =
            !self.mark_index.is_empty() && self.mark_bits.len() == self.all_blocks.len();

        for (idx, block) in self.all_blocks.drain(..).enumerate() {
            let header = unsafe { &mut *block.as_ptr() };

            if header.is_free() {
                // Already free, keep in list
                retained.push(block);
                continue;
            }

            let is_marked = if use_bitmap {
                self.mark_bits.get(idx).is_some_and(|bit| *bit)
            } else {
                matches!(header.color(), Color::Black | Color::Gray)
            };

            if is_marked {
                // Alive - reset to white for next cycle
                header.set_color(Color::White);
                retained.push(block);
            } else {
                // Garbage - notify before freeing
                if let Some(gc_ref) = unsafe { GcRef::from_raw(block.as_ptr()) } {
                    on_collect(gc_ref);
                }

                // Add to free list
                let size = header.size();
                bytes_freed += size;
                objects_freed += 1;

                if size < LARGE_OBJECT_THRESHOLD {
                    if let Some(idx) = Self::size_class_index(size) {
                        unsafe { self.free_lists[idx].push(block) };
                        retained.push(block);
                    }
                } else {
                    // Free large object back to system
                    let layout = Layout::from_size_align(size, ALIGNMENT).unwrap();
                    unsafe { dealloc(block.as_ptr().cast::<u8>(), layout) };
                }
            }
        }

        self.all_blocks = retained;
        self.stats.objects_collected += objects_freed;
        self.stats.bytes_in_use = self.stats.bytes_allocated - bytes_freed;

        // Clean up large objects list
        self.large_objects.retain(|&block| {
            let header = unsafe { &*block.as_ptr() };
            !header.is_free()
        });
    }

    /// Run a full GC cycle
    #[allow(clippy::cast_possible_truncation)] // GC time in nanoseconds fits u64
    pub fn collect<F>(&mut self, root_enumerator: F)
    where
        F: FnOnce(&mut dyn FnMut(GcRef)),
    {
        self.collect_with_callback(root_enumerator, |_| {});
    }

    /// Run a full GC cycle with callback for collected objects
    ///
    /// The `on_collect` callback is invoked for each WHITE object before it is freed,
    /// allowing weak references to be cleared and finalizers to be queued.
    #[allow(clippy::cast_possible_truncation)] // GC time in nanoseconds fits u64
    pub fn collect_with_callback<F, C>(&mut self, root_enumerator: F, on_collect: C)
    where
        F: FnOnce(&mut dyn FnMut(GcRef)),
        C: FnMut(GcRef),
    {
        use std::time::Instant;
        let start = Instant::now();

        // Prepare mark bitmap + index map for this cycle
        self.mark_bits.resize(self.all_blocks.len(), false);
        self.mark_bits.fill(false);
        self.mark_index.clear();
        self.mark_index.reserve(self.all_blocks.len());
        for (idx, block) in self.all_blocks.iter().enumerate() {
            self.mark_index.insert(block.as_ptr(), idx);
        }

        // Mark phase: enumerate roots
        let heap_ptr = std::ptr::from_mut(self);
        root_enumerator(&mut |root| {
            unsafe { (*heap_ptr).mark(root) };
        });

        // Process gray worklist (simplified - real impl needs trace callbacks)
        // For now we just mark direct roots, no child traversal
        self.gray_worklist.clear();

        // Sweep phase with callback
        self.sweep_with_callback(on_collect);

        // Clear bitmap state
        self.mark_index.clear();
        self.mark_bits.clear();

        // Update stats
        self.stats.gc_cycles += 1;
        self.stats.gc_time_ns += start.elapsed().as_nanos() as u64;

        // Adjust threshold based on live data
        self.gc_threshold = (self.stats.bytes_in_use * 2).max(1024 * 1024);
    }

    /// Check if GC should run
    #[must_use]
    pub fn should_collect(&self) -> bool {
        self.stats.bytes_allocated >= self.gc_threshold
    }

    /// Get current statistics
    #[must_use]
    pub fn stats(&self) -> &GcStats {
        &self.stats
    }

    /// Total bytes allocated
    #[must_use]
    pub fn bytes_allocated(&self) -> usize {
        self.stats.bytes_allocated
    }
}

impl Default for Heap {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Heap {
    fn drop(&mut self) {
        // Free all remaining blocks
        for block in &self.all_blocks {
            let header = unsafe { &*block.as_ptr() };
            let size = header.size();
            if let Ok(layout) = Layout::from_size_align(size, ALIGNMENT) {
                unsafe { dealloc(block.as_ptr().cast::<u8>(), layout) };
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_size() {
        assert_eq!(std::mem::size_of::<GcHeader>(), HEADER_SIZE);
    }

    #[test]
    fn test_color_transitions() {
        let mut header = GcHeader::new(TypeTag::Object, 32);
        assert_eq!(header.color(), Color::White);

        header.set_color(Color::Gray);
        assert_eq!(header.color(), Color::Gray);

        header.set_color(Color::Black);
        assert_eq!(header.color(), Color::Black);

        header.set_color(Color::White);
        assert_eq!(header.color(), Color::White);
    }

    #[test]
    fn test_heap_alloc() {
        let mut heap = Heap::new();

        let obj = heap.alloc(TypeTag::Object, 64);
        assert!(obj.is_some());

        let obj = obj.unwrap();
        assert_eq!(obj.header().type_tag(), TypeTag::Object);
        assert_eq!(obj.header().color(), Color::White);
    }

    #[test]
    fn test_free_list() {
        let mut list = FreeList::new(64);
        assert!(list.is_empty());

        // Allocate and free a block
        let layout = Layout::from_size_align(64, ALIGNMENT).unwrap();
        let ptr = unsafe { alloc(layout) };
        #[allow(clippy::cast_ptr_alignment)] // Test allocation is properly aligned
        let block = NonNull::new(ptr.cast::<GcHeader>()).unwrap();

        unsafe {
            *block.as_ptr() = GcHeader::new(TypeTag::Object, 64);
            list.push(block);
        }

        assert!(!list.is_empty());
        assert_eq!(list.len(), 1);

        let popped = list.pop();
        assert!(popped.is_some());
        assert!(list.is_empty());

        // Clean up
        unsafe { dealloc(ptr, layout) };
    }

    #[test]
    fn test_gc_cycle() {
        let mut heap = Heap::new();

        // Allocate some objects
        let obj1 = heap.alloc(TypeTag::Object, 32);
        let obj2 = heap.alloc(TypeTag::Object, 32);
        let _obj3 = heap.alloc(TypeTag::Object, 32); // Not rooted

        assert_eq!(heap.stats().objects_allocated, 3);

        // Run GC with obj1 and obj2 as roots
        heap.collect(|mark| {
            if let Some(o) = obj1 {
                mark(o);
            }
            if let Some(o) = obj2 {
                mark(o);
            }
        });

        // obj3 should have been collected
        assert_eq!(heap.stats().objects_collected, 1);
    }

    #[test]
    fn test_large_object() {
        let mut heap = Heap::new();

        let obj = heap.alloc(TypeTag::Buffer, 8192);
        assert!(obj.is_some());

        let obj = obj.unwrap();
        assert!(obj.header().size() >= 8192);
    }

    #[test]
    fn test_size_classes() {
        // Verify size class coverage
        assert_eq!(Heap::size_class_index(32), Some(0));
        assert_eq!(Heap::size_class_index(64), Some(1));
        assert_eq!(Heap::size_class_index(100), Some(2)); // Rounds up to 128
        assert_eq!(Heap::size_class_index(4096), Some(7));
        assert_eq!(Heap::size_class_index(5000), None); // Large object
    }

    #[test]
    fn test_stats() {
        let mut heap = Heap::new();

        heap.alloc(TypeTag::Object, 32);
        heap.alloc(TypeTag::Object, 64);

        assert!(heap.stats().bytes_allocated > 0);
        assert_eq!(heap.stats().objects_allocated, 2);
    }
}
