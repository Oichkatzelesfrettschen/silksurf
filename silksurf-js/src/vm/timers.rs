//! Timer queue for setTimeout, setInterval, requestAnimationFrame.
//!
//! Timers are sorted by deadline. The event loop drains expired timers
//! each iteration. Timer IDs are monotonically increasing u32s.

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::time::{Duration, Instant};

use super::value::Value;

/// A scheduled timer entry.
#[derive(Clone)]
struct TimerEntry {
    id: u32,
    deadline: Instant,
    callback: Value,
    interval_ms: Option<u64>, // Some(ms) for setInterval, None for setTimeout
    cancelled: bool,
}

impl PartialEq for TimerEntry {
    fn eq(&self, other: &Self) -> bool {
        self.deadline == other.deadline && self.id == other.id
    }
}

impl Eq for TimerEntry {}

impl PartialOrd for TimerEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TimerEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap is a max-heap; we want min-deadline first, so reverse
        other
            .deadline
            .cmp(&self.deadline)
            .then_with(|| other.id.cmp(&self.id))
    }
}

/// Timer queue managing setTimeout, setInterval, requestAnimationFrame.
pub struct TimerQueue {
    heap: BinaryHeap<TimerEntry>,
    next_id: u32,
    /// Callbacks for requestAnimationFrame (drained once per frame).
    raf_callbacks: Vec<(u32, Value)>,
    next_raf_id: u32,
}

impl TimerQueue {
    pub fn new() -> Self {
        Self {
            heap: BinaryHeap::new(),
            next_id: 1,
            raf_callbacks: Vec::new(),
            next_raf_id: 1,
        }
    }

    /// Schedule a setTimeout. Returns timer ID.
    pub fn set_timeout(&mut self, callback: Value, delay_ms: u64) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.heap.push(TimerEntry {
            id,
            deadline: Instant::now() + Duration::from_millis(delay_ms),
            callback,
            interval_ms: None,
            cancelled: false,
        });
        id
    }

    /// Schedule a setInterval. Returns timer ID.
    pub fn set_interval(&mut self, callback: Value, interval_ms: u64) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.heap.push(TimerEntry {
            id,
            deadline: Instant::now() + Duration::from_millis(interval_ms.max(1)),
            callback,
            interval_ms: Some(interval_ms.max(1)),
            cancelled: false,
        });
        id
    }

    /// Cancel a timer by ID (works for both setTimeout and setInterval).
    pub fn clear_timer(&mut self, id: u32) {
        // Mark as cancelled; will be skipped when popped
        // We can't efficiently remove from BinaryHeap, so lazy deletion
        for entry in self.heap.iter() {
            if entry.id == id {
                // BinaryHeap doesn't allow mutable access to elements directly,
                // so we rebuild with the entry marked cancelled
                let mut entries: Vec<TimerEntry> = self.heap.drain().collect();
                for e in &mut entries {
                    if e.id == id {
                        e.cancelled = true;
                    }
                }
                self.heap = entries.into_iter().collect();
                return;
            }
        }
    }

    /// Schedule a requestAnimationFrame callback. Returns ID.
    pub fn request_animation_frame(&mut self, callback: Value) -> u32 {
        let id = self.next_raf_id;
        self.next_raf_id += 1;
        self.raf_callbacks.push((id, callback));
        id
    }

    /// Cancel a requestAnimationFrame by ID.
    pub fn cancel_animation_frame(&mut self, id: u32) {
        self.raf_callbacks.retain(|(raf_id, _)| *raf_id != id);
    }

    /// Drain all expired timers. Returns callbacks to execute.
    /// Re-queues interval timers.
    pub fn drain_expired(&mut self) -> Vec<Value> {
        let now = Instant::now();
        let mut callbacks = Vec::new();

        while let Some(entry) = self.heap.peek() {
            if entry.deadline > now {
                break;
            }
            // UNWRAP-OK: peek() returned Some on the line above, so the heap is non-empty
            // and pop() is guaranteed to return Some.
            let entry = self.heap.pop().unwrap();
            if entry.cancelled {
                continue;
            }
            callbacks.push(entry.callback.clone());

            // Re-queue interval timers
            if let Some(interval_ms) = entry.interval_ms {
                self.heap.push(TimerEntry {
                    id: entry.id,
                    deadline: now + Duration::from_millis(interval_ms),
                    callback: entry.callback,
                    interval_ms: entry.interval_ms,
                    cancelled: false,
                });
            }
        }

        callbacks
    }

    /// Drain all requestAnimationFrame callbacks (called once per frame).
    pub fn drain_raf(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.raf_callbacks)
            .into_iter()
            .map(|(_, cb)| cb)
            .collect()
    }

    /// Time until the next timer fires (for sleep/poll).
    pub fn next_deadline(&self) -> Option<Duration> {
        self.heap.peek().map(|entry| {
            let now = Instant::now();
            if entry.deadline > now {
                entry.deadline - now
            } else {
                Duration::ZERO
            }
        })
    }

    /// Check if there are any pending timers or rAF callbacks.
    pub fn has_pending(&self) -> bool {
        !self.heap.is_empty() || !self.raf_callbacks.is_empty()
    }
}

impl Default for TimerQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::value::Value;

    #[test]
    fn test_set_timeout() {
        let mut q = TimerQueue::new();
        let id = q.set_timeout(Value::Number(1.0), 0);
        assert_eq!(id, 1);
        // With 0ms delay, should be immediately drainable
        std::thread::sleep(Duration::from_millis(1));
        let callbacks = q.drain_expired();
        assert_eq!(callbacks.len(), 1);
    }

    #[test]
    fn test_clear_timeout() {
        let mut q = TimerQueue::new();
        let id = q.set_timeout(Value::Number(1.0), 0);
        q.clear_timer(id);
        std::thread::sleep(Duration::from_millis(1));
        let callbacks = q.drain_expired();
        assert_eq!(callbacks.len(), 0);
    }

    #[test]
    fn test_set_interval() {
        let mut q = TimerQueue::new();
        let _id = q.set_interval(Value::Number(1.0), 1);
        std::thread::sleep(Duration::from_millis(5));
        let callbacks = q.drain_expired();
        assert!(!callbacks.is_empty());
        // Should re-queue
        assert!(q.has_pending());
    }

    #[test]
    fn test_raf() {
        let mut q = TimerQueue::new();
        let id = q.request_animation_frame(Value::Number(1.0));
        assert_eq!(id, 1);
        let callbacks = q.drain_raf();
        assert_eq!(callbacks.len(), 1);
        // Drained, no more pending rAF
        assert!(q.drain_raf().is_empty());
    }

    #[test]
    fn test_ordering() {
        let mut q = TimerQueue::new();
        // Later timer first
        q.set_timeout(Value::Number(2.0), 100);
        // Earlier timer second
        q.set_timeout(Value::Number(1.0), 0);
        std::thread::sleep(Duration::from_millis(1));
        let callbacks = q.drain_expired();
        // Only the 0ms timer should have fired
        assert_eq!(callbacks.len(), 1);
        assert!(matches!(callbacks[0], Value::Number(n) if n == 1.0));
    }
}
