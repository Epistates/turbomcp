//! Lock-free data structures for high-performance concurrent access
//!
//! This module provides lock-free alternatives to standard collections
//! for maximum performance in high-concurrency scenarios.

use crossbeam::epoch::{self, Atomic, Owned};
use crossbeam::queue::{ArrayQueue, SegQueue};
use dashmap::DashMap;
use parking_lot::RwLock;
use std::hash::Hash;
use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Lock-free bounded SPSC (Single Producer Single Consumer) queue
/// Optimized for message passing between two threads
#[derive(Debug)]
pub struct SpscQueue<T> {
    inner: ArrayQueue<T>,
    capacity: usize,
}

impl<T> SpscQueue<T> {
    /// Create a new SPSC queue with the given capacity
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: ArrayQueue::new(capacity),
            capacity,
        }
    }

    /// Try to push an item to the queue
    #[inline]
    pub fn push(&self, item: T) -> Result<(), T> {
        self.inner.push(item)
    }

    /// Try to pop an item from the queue
    #[inline]
    pub fn pop(&self) -> Option<T> {
        self.inner.pop()
    }

    /// Get the number of items in the queue
    #[inline]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Check if the queue is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Check if the queue is full
    #[inline]
    pub fn is_full(&self) -> bool {
        self.inner.len() >= self.capacity
    }
}

/// Lock-free unbounded MPMC (Multi Producer Multi Consumer) queue
/// Suitable for work-stealing and task distribution
#[derive(Debug)]
pub struct MpmcQueue<T> {
    inner: SegQueue<T>,
    len: AtomicUsize,
}

impl<T> MpmcQueue<T> {
    /// Create a new unbounded MPMC queue
    pub fn new() -> Self {
        Self {
            inner: SegQueue::new(),
            len: AtomicUsize::new(0),
        }
    }

    /// Push an item to the queue
    #[inline]
    pub fn push(&self, item: T) {
        self.inner.push(item);
        self.len.fetch_add(1, Ordering::Relaxed);
    }

    /// Try to pop an item from the queue
    #[inline]
    pub fn pop(&self) -> Option<T> {
        let item = self.inner.pop();
        if item.is_some() {
            self.len.fetch_sub(1, Ordering::Relaxed);
        }
        item
    }

    /// Get approximate length
    #[inline]
    pub fn len(&self) -> usize {
        self.len.load(Ordering::Relaxed)
    }

    /// Check if the queue is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

impl<T> Default for MpmcQueue<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Lock-free stack using Treiber's algorithm
/// Provides LIFO semantics with concurrent access
#[derive(Debug)]
pub struct LockFreeStack<T> {
    head: Atomic<Node<T>>,
    _marker: PhantomData<T>,
}

#[derive(Debug)]
struct Node<T> {
    data: T,
    next: Atomic<Node<T>>,
}

impl<T> LockFreeStack<T> {
    /// Create a new lock-free stack
    pub fn new() -> Self {
        Self {
            head: Atomic::null(),
            _marker: PhantomData,
        }
    }

    /// Push an item onto the stack
    pub fn push(&self, data: T) {
        let guard = &epoch::pin();
        let mut new_node = Owned::new(Node {
            data,
            next: Atomic::null(),
        });

        loop {
            let head = self.head.load(Ordering::Acquire, guard);
            new_node.next.store(head, Ordering::Relaxed);

            match self.head.compare_exchange(
                head,
                new_node,
                Ordering::Release,
                Ordering::Acquire,
                guard,
            ) {
                Ok(_) => break,
                Err(e) => new_node = e.new,
            }
        }
    }

    /// Pop an item from the stack
    pub fn pop(&self) -> Option<T> {
        let guard = &epoch::pin();
        loop {
            let head = self.head.load(Ordering::Acquire, guard);
            // SAFETY: head is protected by epoch guard, ensuring memory remains valid
            // throughout this scope. crossbeam's epoch GC prevents use-after-free.
            match unsafe { head.as_ref() } {
                None => return None,
                Some(h) => {
                    let next = h.next.load(Ordering::Acquire, guard);
                    if self
                        .head
                        .compare_exchange(head, next, Ordering::Release, Ordering::Acquire, guard)
                        .is_ok()
                    {
                        // SAFETY: We successfully CAS'd the head pointer, so we now own this node.
                        // - guard.defer_destroy schedules safe deallocation after all guards are dropped
                        // - ptr::read moves the value without calling drop on the original location
                        // - No other thread can access this node after successful CAS
                        unsafe {
                            guard.defer_destroy(head);
                            return Some(std::ptr::read(&h.data));
                        }
                    }
                }
            }
        }
    }

    /// Check if the stack is empty
    pub fn is_empty(&self) -> bool {
        let guard = &epoch::pin();
        self.head.load(Ordering::Acquire, guard).is_null()
    }
}

impl<T> Default for LockFreeStack<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Drop for LockFreeStack<T> {
    fn drop(&mut self) {
        while self.pop().is_some() {}
    }
}

/// Lock-free concurrent hashmap optimized for read-heavy workloads
#[derive(Debug, Clone)]
pub struct LockFreeMap<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    inner: Arc<DashMap<K, V>>,
}

impl<K, V> LockFreeMap<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    /// Create a new lock-free map
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DashMap::new()),
        }
    }

    /// Create with specified capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Arc::new(DashMap::with_capacity(capacity)),
        }
    }

    /// Insert a key-value pair
    #[inline]
    pub fn insert(&self, key: K, value: V) -> Option<V> {
        self.inner.insert(key, value)
    }

    /// Get a value by key
    #[inline]
    pub fn get(&self, key: &K) -> Option<V> {
        self.inner.get(key).map(|v| v.clone())
    }

    /// Remove a key-value pair
    #[inline]
    pub fn remove(&self, key: &K) -> Option<(K, V)> {
        self.inner.remove(key)
    }

    /// Check if the map contains a key
    #[inline]
    pub fn contains_key(&self, key: &K) -> bool {
        self.inner.contains_key(key)
    }

    /// Get the number of entries
    #[inline]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Check if the map is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Clear all entries
    #[inline]
    pub fn clear(&self) {
        self.inner.clear()
    }
}

impl<K, V> Default for LockFreeMap<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    fn default() -> Self {
        Self::new()
    }
}

/// Optimized ring buffer for single-writer multiple-reader scenarios
#[derive(Debug)]
pub struct RingBuffer<T> {
    buffer: Arc<Vec<RwLock<Option<T>>>>,
    capacity: usize,
    write_pos: AtomicUsize,
    read_pos: AtomicUsize,
}

impl<T: Clone> RingBuffer<T> {
    /// Create a new ring buffer with the given capacity
    pub fn new(capacity: usize) -> Self {
        let mut buffer = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            buffer.push(RwLock::new(None));
        }

        Self {
            buffer: Arc::new(buffer),
            capacity,
            write_pos: AtomicUsize::new(0),
            read_pos: AtomicUsize::new(0),
        }
    }

    /// Write an item to the buffer
    pub fn write(&self, item: T) -> bool {
        let write_pos = self.write_pos.load(Ordering::Acquire);
        let next_write = (write_pos + 1) % self.capacity;
        let read_pos = self.read_pos.load(Ordering::Acquire);

        // Check if buffer is full
        if next_write == read_pos {
            return false;
        }

        // Write the item
        let mut slot = self.buffer[write_pos].write();
        *slot = Some(item);
        drop(slot);

        // Update write position
        self.write_pos.store(next_write, Ordering::Release);
        true
    }

    /// Read an item from the buffer
    pub fn read(&self) -> Option<T> {
        let read_pos = self.read_pos.load(Ordering::Acquire);
        let write_pos = self.write_pos.load(Ordering::Acquire);

        // Check if buffer is empty
        if read_pos == write_pos {
            return None;
        }

        // Read and take the item
        let mut slot = self.buffer[read_pos].write();
        let item = slot.take();
        drop(slot);

        if item.is_some() {
            // Update read position
            let next_read = (read_pos + 1) % self.capacity;
            self.read_pos.store(next_read, Ordering::Release);
        }

        item
    }

    /// Get the number of items in the buffer
    pub fn len(&self) -> usize {
        let write_pos = self.write_pos.load(Ordering::Relaxed);
        let read_pos = self.read_pos.load(Ordering::Relaxed);

        if write_pos >= read_pos {
            write_pos - read_pos
        } else {
            self.capacity - read_pos + write_pos
        }
    }

    /// Check if the buffer is empty
    pub fn is_empty(&self) -> bool {
        let write_pos = self.write_pos.load(Ordering::Relaxed);
        let read_pos = self.read_pos.load(Ordering::Relaxed);
        write_pos == read_pos
    }

    /// Check if the buffer is full
    pub fn is_full(&self) -> bool {
        self.len() >= self.capacity - 1 // Reserve one slot to distinguish full from empty
    }
}

/// Atomic counter for high-performance counting
#[derive(Debug)]
pub struct AtomicCounter {
    value: AtomicUsize,
}

impl AtomicCounter {
    /// Create a new atomic counter
    pub const fn new(initial: usize) -> Self {
        Self {
            value: AtomicUsize::new(initial),
        }
    }

    /// Increment the counter
    #[inline]
    pub fn increment(&self) -> usize {
        self.value.fetch_add(1, Ordering::Relaxed)
    }

    /// Decrement the counter
    #[inline]
    pub fn decrement(&self) -> usize {
        self.value.fetch_sub(1, Ordering::Relaxed)
    }

    /// Get the current value
    #[inline]
    pub fn get(&self) -> usize {
        self.value.load(Ordering::Relaxed)
    }

    /// Set the value
    #[inline]
    pub fn set(&self, value: usize) {
        self.value.store(value, Ordering::Relaxed);
    }

    /// Reset to zero
    #[inline]
    pub fn reset(&self) {
        self.value.store(0, Ordering::Relaxed);
    }
}

impl Default for AtomicCounter {
    fn default() -> Self {
        Self::new(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_spsc_queue() {
        let queue = Arc::new(SpscQueue::new(10));
        let q1 = queue.clone();
        let q2 = queue.clone();

        // Producer thread
        let producer = thread::spawn(move || {
            for i in 0..10 {
                while q1.push(i).is_err() {
                    thread::yield_now();
                }
            }
        });

        // Consumer thread
        let consumer = thread::spawn(move || {
            let mut items = Vec::new();
            while items.len() < 10 {
                if let Some(item) = q2.pop() {
                    items.push(item);
                }
            }
            items
        });

        producer.join().unwrap();
        let items = consumer.join().unwrap();

        assert_eq!(items.len(), 10);
        for (i, &item) in items.iter().enumerate() {
            assert_eq!(item, i);
        }
    }

    #[test]
    fn test_mpmc_queue() {
        let queue = Arc::new(MpmcQueue::new());
        let mut handles = Vec::new();

        // Multiple producers
        for i in 0..4 {
            let q = queue.clone();
            handles.push(thread::spawn(move || {
                for j in 0..25 {
                    q.push(i * 25 + j);
                }
            }));
        }

        // Wait for producers
        for h in handles {
            h.join().unwrap();
        }

        // Consume all items
        let mut items = Vec::new();
        while let Some(item) = queue.pop() {
            items.push(item);
        }

        assert_eq!(items.len(), 100);
        items.sort();
        for (i, &item) in items.iter().enumerate() {
            assert_eq!(item, i);
        }
    }

    #[test]
    fn test_lock_free_stack() {
        let stack = Arc::new(LockFreeStack::new());
        let mut handles = Vec::new();

        // Multiple threads pushing
        for i in 0..4 {
            let s = stack.clone();
            handles.push(thread::spawn(move || {
                for j in 0..25 {
                    s.push(i * 25 + j);
                }
            }));
        }

        // Wait for pushers
        for h in handles {
            h.join().unwrap();
        }

        // Pop all items
        let mut items = Vec::new();
        while let Some(item) = stack.pop() {
            items.push(item);
        }

        assert_eq!(items.len(), 100);
    }

    #[test]
    fn test_lock_free_map() {
        let map = Arc::new(LockFreeMap::new());
        let mut handles = Vec::new();

        // Multiple threads inserting
        for i in 0..4 {
            let m = map.clone();
            handles.push(thread::spawn(move || {
                for j in 0..25 {
                    let key = i * 25 + j;
                    m.insert(key, format!("value_{}", key));
                }
            }));
        }

        // Wait for inserters
        for h in handles {
            h.join().unwrap();
        }

        // Verify all values
        assert_eq!(map.len(), 100);
        for i in 0..100 {
            assert_eq!(map.get(&i), Some(format!("value_{}", i)));
        }
    }

    #[test]
    fn test_ring_buffer() {
        let buffer = RingBuffer::new(10);

        // Fill buffer (can only hold 9 items because we reserve 1 slot)
        for i in 0..9 {
            assert!(buffer.write(i));
        }

        // Buffer should be full (9 items in 10-slot buffer with 1 reserved)
        assert_eq!(buffer.len(), 9);
        assert!(buffer.is_full());

        // Try to write one more - should fail
        assert!(!buffer.write(99));

        // Read some items
        for i in 0..5 {
            assert_eq!(buffer.read(), Some(i));
        }

        // Now we can write more items (5 slots available)
        for i in 9..14 {
            assert!(buffer.write(i));
        }

        // Read remaining items
        let mut items = Vec::new();
        while let Some(item) = buffer.read() {
            items.push(item);
        }

        assert_eq!(items, vec![5, 6, 7, 8, 9, 10, 11, 12, 13]);
    }

    #[test]
    fn test_atomic_counter() {
        let counter = Arc::new(AtomicCounter::new(0));
        let mut handles = Vec::new();

        // Multiple threads incrementing
        for _ in 0..10 {
            let c = counter.clone();
            handles.push(thread::spawn(move || {
                for _ in 0..1000 {
                    c.increment();
                }
            }));
        }

        // Wait for all threads
        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(counter.get(), 10000);
    }
}
