//! Zero-copy message processing with minimal allocations
//!
//! This module provides zero-allocation message handling using `bytes::Bytes`
//! for maximum throughput and minimal memory overhead.
//!
//! ## When to Use ZeroCopyMessage
//!
//! **Most users should use [`Message`](crate::Message) instead.**
//!
//! `ZeroCopyMessage` is designed for extreme performance scenarios where:
//! - You process **millions of messages per second**
//! - **Every allocation matters** for your performance profile
//! - You can **defer deserialization** until absolutely necessary
//! - You're willing to **trade ergonomics for performance**
//!
//! ### Message vs ZeroCopyMessage
//!
//! | Feature | [`Message`](crate::Message) | `ZeroCopyMessage` |
//! |---------|---------|------------------|
//! | Ergonomics | ✅ Excellent | ⚠️ Manual |
//! | Memory | ✅ Good | ✅ Optimal |
//! | Deserialization | Eager | Lazy |
//! | Multiple formats | ✅ JSON/CBOR/MessagePack | JSON only |
//! | ID storage | Stack/String | Arc (shared) |
//! | Use case | General purpose | Ultra-high throughput |
//!
//! ### Example Usage
//!
//! ```rust
//! use turbomcp_protocol::zero_copy::{ZeroCopyMessage, MessageId};
//! use bytes::Bytes;
//!
//! // Create from raw bytes (no allocation)
//! let payload = Bytes::from(r#"{"method": "test", "id": 1}"#);
//! let mut msg = ZeroCopyMessage::from_bytes(MessageId::from("req-1"), payload);
//!
//! // Lazy parsing - returns RawValue without full deserialization
//! let raw = msg.parse_json_lazy()?;
//! assert!(raw.get().contains("method"));
//!
//! // Full deserialization when needed
//! let data: serde_json::Value = msg.deserialize()?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! **Performance Tip**: Use `ZeroCopyMessage` in hot paths where you need to
//! route messages based on metadata but don't need to parse the payload.

use bytes::{BufMut, Bytes, BytesMut};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use std::fmt;
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::types::{ContentType, Timestamp};

/// Zero-copy message with lazy deserialization
#[derive(Debug, Clone)]
pub struct ZeroCopyMessage {
    /// Message ID - using Arc for cheap cloning
    pub id: Arc<MessageId>,

    /// Raw message payload - zero-copy bytes
    pub payload: Bytes,

    /// Lazy-parsed JSON value for deferred deserialization
    pub lazy_json: Option<Box<RawValue>>,

    /// Message metadata
    pub metadata: MessageMetadata,
}

/// Optimized message ID with Arc sharing
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MessageId {
    /// String ID with Arc for sharing
    String(Arc<str>),
    /// Numeric ID (stack-allocated)
    Number(i64),
    /// UUID (stack-allocated)
    Uuid(Uuid),
}

/// Lightweight message metadata
#[derive(Debug, Clone)]
pub struct MessageMetadata {
    /// Creation timestamp
    pub created_at: Timestamp,
    /// Content type
    pub content_type: ContentType,
    /// Message size in bytes
    pub size: usize,
    /// Optional correlation ID (Arc for sharing)
    pub correlation_id: Option<Arc<str>>,
}

impl ZeroCopyMessage {
    /// Create a new zero-copy message from bytes
    #[inline]
    pub fn from_bytes(id: MessageId, payload: Bytes) -> Self {
        let size = payload.len();
        Self {
            id: Arc::new(id),
            payload: payload.clone(),
            lazy_json: None,
            metadata: MessageMetadata {
                created_at: Timestamp::now(),
                content_type: ContentType::Json,
                size,
                correlation_id: None,
            },
        }
    }

    /// Create from a JSON value with zero-copy optimization
    pub fn from_json<T: Serialize>(id: MessageId, value: &T) -> Result<Self> {
        // Use a reusable buffer pool in production
        let mut buffer = BytesMut::with_capacity(1024);

        // Serialize directly to bytes
        serde_json::to_writer((&mut buffer).writer(), value)
            .map_err(|e| Error::serialization(e.to_string()))?;

        let payload = buffer.freeze();
        let size = payload.len();

        Ok(Self {
            id: Arc::new(id),
            payload,
            lazy_json: None,
            metadata: MessageMetadata {
                created_at: Timestamp::now(),
                content_type: ContentType::Json,
                size,
                correlation_id: None,
            },
        })
    }

    /// Parse JSON lazily - only when needed
    #[inline]
    pub fn parse_json_lazy(&mut self) -> Result<&RawValue> {
        if self.lazy_json.is_none() {
            // Parse without deserializing the full structure
            let raw: Box<RawValue> = serde_json::from_slice(&self.payload)
                .map_err(|e| Error::serialization(format!("JSON parse error: {}", e)))?;

            // Store the parsed raw value
            self.lazy_json = Some(raw);
        }

        Ok(self.lazy_json.as_ref().unwrap())
    }

    /// Deserialize a specific type from the message with SIMD acceleration when available
    #[inline]
    pub fn deserialize<T: for<'de> Deserialize<'de>>(&self) -> Result<T> {
        #[cfg(feature = "simd")]
        {
            // Use simd-json for faster parsing when payload is large enough
            if self.payload.len() >= 64 {
                // Clone to mutable buffer for SIMD parsing
                let mut buffer = self.payload.to_vec();
                simd_json::from_slice(&mut buffer)
                    .map_err(|e| Error::serialization(format!("SIMD deserialize error: {}", e)))
            } else {
                // Fall back to standard parsing for small payloads
                serde_json::from_slice(&self.payload)
                    .map_err(|e| Error::serialization(format!("Deserialization error: {}", e)))
            }
        }
        #[cfg(not(feature = "simd"))]
        {
            serde_json::from_slice(&self.payload)
                .map_err(|e| Error::serialization(format!("Deserialization error: {}", e)))
        }
    }

    /// Get a zero-copy view of the payload
    #[inline]
    pub fn payload_slice(&self) -> &[u8] {
        &self.payload
    }

    /// Clone the message cheaply (Arc increments only)
    #[inline]
    pub fn cheap_clone(&self) -> Self {
        Self {
            id: Arc::clone(&self.id),
            payload: self.payload.clone(), // Bytes is already Arc-based
            lazy_json: self.lazy_json.clone(),
            metadata: self.metadata.clone(),
        }
    }
}

/// Buffer pool for reusing allocations
#[derive(Debug)]
pub struct BufferPool {
    /// Pool of reusable buffers
    buffers: crossbeam::queue::ArrayQueue<BytesMut>,
    /// Default buffer capacity
    capacity: usize,
}

impl BufferPool {
    /// Create a new buffer pool
    pub fn new(size: usize, capacity: usize) -> Self {
        let buffers = crossbeam::queue::ArrayQueue::new(size);

        // Pre-allocate buffers
        for _ in 0..size {
            let _ = buffers.push(BytesMut::with_capacity(capacity));
        }

        Self { buffers, capacity }
    }

    /// Get a buffer from the pool or create a new one
    #[inline]
    pub fn acquire(&self) -> BytesMut {
        self.buffers
            .pop()
            .unwrap_or_else(|| BytesMut::with_capacity(self.capacity))
    }

    /// Return a buffer to the pool for reuse
    #[inline]
    pub fn release(&self, mut buffer: BytesMut) {
        buffer.clear();
        let _ = self.buffers.push(buffer);
    }
}

/// Zero-copy message batch for efficient bulk processing
#[derive(Debug)]
pub struct MessageBatch {
    /// Contiguous buffer containing all messages
    pub buffer: Bytes,
    /// Offsets and lengths of individual messages
    pub messages: Vec<(usize, usize)>,
    /// Shared message IDs
    pub ids: Vec<Arc<MessageId>>,
}

impl MessageBatch {
    /// Create a new message batch
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: Bytes::new(),
            messages: Vec::with_capacity(capacity),
            ids: Vec::with_capacity(capacity),
        }
    }

    /// Add a message to the batch
    pub fn add(&mut self, id: MessageId, payload: Bytes) {
        let offset = self.buffer.len();
        let length = payload.len();

        // Extend the buffer
        let mut buffer = BytesMut::from(self.buffer.as_ref());
        buffer.extend_from_slice(&payload);
        self.buffer = buffer.freeze();

        // Store offset and length
        self.messages.push((offset, length));
        self.ids.push(Arc::new(id));
    }

    /// Get a zero-copy view of a message
    #[inline]
    pub fn get(&self, index: usize) -> Option<Bytes> {
        self.messages
            .get(index)
            .map(|(offset, length)| self.buffer.slice(*offset..*offset + *length))
    }

    /// Iterate over messages without copying
    pub fn iter(&self) -> impl Iterator<Item = (&Arc<MessageId>, Bytes)> + '_ {
        self.ids
            .iter()
            .zip(self.messages.iter())
            .map(move |(id, (offset, length))| (id, self.buffer.slice(*offset..*offset + *length)))
    }
}

/// Fast utilities for message processing with SIMD acceleration
pub mod fast {
    /// Fast UTF-8 validation with SIMD when available
    #[inline]
    pub fn validate_utf8_fast(bytes: &[u8]) -> bool {
        #[cfg(feature = "simd")]
        {
            // Use SIMD-accelerated validation for larger inputs
            if bytes.len() >= 64 {
                simdutf8::basic::from_utf8(bytes).is_ok()
            } else {
                std::str::from_utf8(bytes).is_ok()
            }
        }
        #[cfg(not(feature = "simd"))]
        {
            std::str::from_utf8(bytes).is_ok()
        }
    }

    /// Fast JSON boundary detection with optimized scanning
    #[inline]
    pub fn find_json_boundaries(bytes: &[u8]) -> Vec<usize> {
        let mut boundaries = Vec::new();
        let mut depth = 0;
        let mut in_string = false;
        let mut escaped = false;

        // Optimized boundary detection with proper string handling
        for (i, &byte) in bytes.iter().enumerate() {
            if escaped {
                escaped = false;
                continue;
            }

            match byte {
                b'\\' if in_string => escaped = true,
                b'"' if !escaped => in_string = !in_string,
                b'{' | b'[' if !in_string => depth += 1,
                b'}' | b']' if !in_string => {
                    depth -= 1;
                    if depth == 0 {
                        boundaries.push(i + 1);
                    }
                }
                _ => {}
            }
        }

        boundaries
    }

    /// SIMD-accelerated JSON validation
    #[cfg(feature = "simd")]
    #[inline]
    pub fn validate_json_fast(bytes: &[u8]) -> bool {
        if bytes.len() >= 64 {
            // Use simd-json for validation
            let mut owned = bytes.to_vec();
            simd_json::to_borrowed_value(&mut owned).is_ok()
        } else {
            // Fall back to standard validation for small inputs
            serde_json::from_slice::<serde_json::Value>(bytes).is_ok()
        }
    }

    /// Standard JSON validation (non-SIMD fallback)
    ///
    /// Validates JSON syntax using serde_json's parser.
    #[cfg(not(feature = "simd"))]
    #[inline]
    pub fn validate_json_fast(bytes: &[u8]) -> bool {
        serde_json::from_slice::<serde_json::Value>(bytes).is_ok()
    }
}

/// Memory-mapped file support for efficient large file processing
#[cfg(feature = "mmap")]
pub mod mmap {
    use super::*;
    use memmap2::{Mmap, MmapOptions};
    use std::fs::File;
    use std::io;
    use std::ops::Deref;

    // Import security module if available

    /// A memory-mapped message for zero-copy file access
    #[derive(Debug)]
    pub struct MmapMessage {
        /// Message ID
        pub id: Arc<MessageId>,
        /// Memory-mapped data
        pub mmap: Arc<Mmap>,
        /// Offset within the mapped region
        pub offset: usize,
        /// Length of the message data
        pub length: usize,
        /// Message metadata
        pub metadata: MessageMetadata,
    }

    impl MmapMessage {
        /// Create a message from a memory-mapped file (legacy method without security)
        ///
        /// **SECURITY WARNING**: This method bypasses security validation.
        /// Use `from_file_secure` for production systems.
        pub fn from_file(
            id: MessageId,
            path: &Path,
            offset: usize,
            length: Option<usize>,
        ) -> io::Result<Self> {
            // For backwards compatibility, perform basic validation
            let file = File::open(path)?;
            let metadata = file.metadata()?;
            let file_size = metadata.len() as usize;

            // Validate offset
            if offset >= file_size {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Offset exceeds file size",
                ));
            }

            // Calculate actual length
            let actual_length = length.unwrap_or(file_size - offset);
            let actual_length = actual_length.min(file_size - offset);

            // Create memory map
            // SAFETY: file handle is valid and opened for reading. memmap2 provides
            // safe abstractions over POSIX mmap. The resulting mapping is read-only.
            let mmap = unsafe { MmapOptions::new().map(&file)? };

            Ok(Self {
                id: Arc::new(id),
                mmap: Arc::new(mmap),
                offset,
                length: actual_length,
                metadata: MessageMetadata {
                    created_at: Timestamp::now(),
                    content_type: ContentType::Json,
                    size: actual_length,
                    correlation_id: None,
                },
            })
        }

        /// Internal method for creating memory-mapped files after security validation
        #[allow(dead_code)] // Used with mmap feature
        async fn from_file_internal(
            id: MessageId,
            path: &Path,
            offset: usize,
            length: Option<usize>,
        ) -> io::Result<Self> {
            let file = File::open(path)?;
            let metadata = file.metadata()?;
            let file_size = metadata.len() as usize;

            // Validate offset (already validated by security layer, but double-check)
            if offset >= file_size {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Offset exceeds file size",
                ));
            }

            // Calculate actual length
            let actual_length = length.unwrap_or(file_size - offset);
            let actual_length = actual_length.min(file_size - offset);

            // Create memory map
            // SAFETY: file handle is valid and opened for reading. memmap2 provides
            // safe abstractions over POSIX mmap. The resulting mapping is read-only.
            let mmap = unsafe { MmapOptions::new().map(&file)? };

            Ok(Self {
                id: Arc::new(id),
                mmap: Arc::new(mmap),
                offset,
                length: actual_length,
                metadata: MessageMetadata {
                    created_at: Timestamp::now(),
                    content_type: ContentType::Json,
                    size: actual_length,
                    correlation_id: None,
                },
            })
        }

        /// Create a message from a memory-mapped file (ASYNC - Non-blocking!)
        ///
        /// This is the async version of `from_file` that uses `tokio::task::spawn_blocking`
        /// to avoid blocking the async runtime during file I/O operations.
        ///
        /// # Production-Grade Async I/O
        /// - Uses spawn_blocking for CPU-intensive mmap operations
        /// - Maintains same functionality as sync version
        /// - Safe to call from async contexts without blocking
        /// - Proper error propagation and resource cleanup
        pub async fn from_file_async(
            id: MessageId,
            path: &std::path::Path,
            offset: usize,
            length: Option<usize>,
        ) -> std::io::Result<Self> {
            let path = path.to_path_buf(); // Clone for move into spawn_blocking

            tokio::task::spawn_blocking(move || Self::from_file(id, &path, offset, length))
                .await
                .map_err(|join_err| {
                    std::io::Error::other(format!("Async mmap operation failed: {}", join_err))
                })?
        }

        /// Get the message data as a byte slice
        #[inline]
        pub fn data(&self) -> &[u8] {
            &self.mmap[self.offset..self.offset + self.length]
        }

        /// Convert to a Bytes instance for compatibility
        #[inline]
        pub fn to_bytes(&self) -> Bytes {
            Bytes::copy_from_slice(self.data())
        }

        /// Parse JSON lazily from the mapped data
        /// Parse the message data as JSON
        pub fn parse_json<T>(&self) -> Result<T>
        where
            T: for<'de> Deserialize<'de>,
        {
            serde_json::from_slice(self.data())
                .map_err(|e| Error::serialization(format!("JSON parse error: {}", e)))
        }

        /// Get a zero-copy string view if the data is valid UTF-8
        /// Get the message data as a string slice
        pub fn as_str(&self) -> Result<&str> {
            std::str::from_utf8(self.data())
                .map_err(|e| Error::serialization(format!("Invalid UTF-8: {}", e)))
        }
    }

    /// A pool of memory-mapped files for efficient reuse
    #[derive(Debug)]
    pub struct MmapPool {
        /// Cached memory maps
        maps: dashmap::DashMap<std::path::PathBuf, Arc<Mmap>>,
        /// Maximum number of cached maps
        max_size: usize,
    }

    impl MmapPool {
        /// Create a new memory map pool
        pub fn new(max_size: usize) -> Self {
            Self {
                maps: dashmap::DashMap::new(),
                max_size,
            }
        }

        /// Get or create a memory map for a file
        pub fn get_or_create(&self, path: &Path) -> io::Result<Arc<Mmap>> {
            // Check if already cached
            if let Some(mmap) = self.maps.get(path) {
                return Ok(Arc::clone(&*mmap));
            }

            // Create new memory map
            let file = File::open(path)?;
            // SAFETY: file handle is valid and opened for reading. memmap2 provides
            // safe abstractions over POSIX mmap. The resulting mapping is read-only.
            let mmap = unsafe { MmapOptions::new().map(&file)? };
            let mmap = Arc::new(mmap);

            // Cache if under limit
            if self.maps.len() < self.max_size {
                self.maps.insert(path.to_path_buf(), Arc::clone(&mmap));
            }

            Ok(mmap)
        }

        /// Clear the cache
        pub fn clear(&self) {
            self.maps.clear();
        }

        /// Get cache size
        pub fn size(&self) -> usize {
            self.maps.len()
        }
    }

    /// Memory-mapped message batch for processing multiple messages from a file
    #[derive(Debug)]
    pub struct MmapBatch {
        /// Memory-mapped file
        mmap: Arc<Mmap>,
        /// Message boundaries (offset, length)
        messages: Vec<(usize, usize)>,
        /// Message IDs
        ids: Vec<Arc<MessageId>>,
    }

    impl MmapBatch {
        /// Create a batch from a memory-mapped file with JSON lines
        pub fn from_jsonl_file(path: &Path) -> io::Result<Self> {
            let file = File::open(path)?;
            // SAFETY: file handle is valid and opened for reading. memmap2 provides
            // safe abstractions over POSIX mmap. The resulting mapping is read-only.
            let mmap = unsafe { MmapOptions::new().map(&file)? };

            let mut messages = Vec::new();
            let mut ids = Vec::new();
            let mut offset = 0;

            // Parse JSON lines
            for (idx, line) in mmap.split(|&b| b == b'\n').enumerate() {
                if !line.is_empty() {
                    messages.push((offset, line.len()));
                    ids.push(Arc::new(MessageId::Number(idx as i64)));
                }
                offset += line.len() + 1; // +1 for newline
            }

            Ok(Self {
                mmap: Arc::new(mmap),
                messages,
                ids,
            })
        }

        /// Get a message by index
        #[inline]
        pub fn get(&self, index: usize) -> Option<&[u8]> {
            self.messages
                .get(index)
                .map(|(offset, length)| &self.mmap[*offset..*offset + *length])
        }

        /// Iterate over messages
        pub fn iter(&self) -> impl Iterator<Item = (&Arc<MessageId>, &[u8])> + '_ {
            self.ids
                .iter()
                .zip(self.messages.iter())
                .map(move |(id, (offset, length))| {
                    (id, &self.mmap.deref()[*offset..*offset + *length])
                })
        }

        /// Get the number of messages
        pub fn len(&self) -> usize {
            self.messages.len()
        }

        /// Check if batch is empty
        pub fn is_empty(&self) -> bool {
            self.messages.is_empty()
        }

        /// Create a batch from a memory-mapped JSONL file (ASYNC - Non-blocking!)
        ///
        /// This is the async version of `from_jsonl_file` that uses `tokio::task::spawn_blocking`
        /// to avoid blocking the async runtime during file I/O and parsing operations.
        ///
        /// # Production-Grade Async I/O
        /// - Uses spawn_blocking for CPU-intensive mmap and parsing operations
        /// - Maintains same functionality as sync version
        /// - Safe to call from async contexts without blocking
        /// - Proper error propagation and resource cleanup
        pub async fn from_jsonl_file_async(path: &std::path::Path) -> std::io::Result<Self> {
            let path = path.to_path_buf(); // Clone for move into spawn_blocking

            tokio::task::spawn_blocking(move || Self::from_jsonl_file(&path))
                .await
                .map_err(|join_err| {
                    std::io::Error::other(format!(
                        "Async JSONL batch operation failed: {}",
                        join_err
                    ))
                })?
        }
    }
}

/// Memory-mapped file support (safe wrapper when feature is disabled)
#[cfg(not(feature = "mmap"))]
pub mod mmap {
    use super::*;
    use std::fs;
    use std::io;

    /// Fallback implementation using regular file I/O
    #[derive(Debug)]
    pub struct MmapMessage {
        /// Unique message identifier
        pub id: Arc<MessageId>,
        /// Message data as bytes
        pub data: Bytes,
        /// Message metadata
        pub metadata: MessageMetadata,
    }

    impl MmapMessage {
        /// Create a message by reading from a file
        pub fn from_file(
            id: MessageId,
            path: &Path,
            offset: usize,
            length: Option<usize>,
        ) -> io::Result<Self> {
            let data = fs::read(path)?;
            let file_size = data.len();

            if offset >= file_size {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Offset exceeds file size",
                ));
            }

            let actual_length = length.unwrap_or(file_size - offset);
            let actual_length = actual_length.min(file_size - offset);

            let data = Bytes::copy_from_slice(&data[offset..offset + actual_length]);

            Ok(Self {
                id: Arc::new(id),
                data: data.clone(),
                metadata: MessageMetadata {
                    created_at: Timestamp::now(),
                    content_type: ContentType::Json,
                    size: actual_length,
                    correlation_id: None,
                },
            })
        }

        /// Get the message data as a byte slice
        #[inline]
        pub fn data(&self) -> &[u8] {
            &self.data
        }

        /// Convert the message data to Bytes
        #[inline]
        pub fn to_bytes(&self) -> Bytes {
            self.data.clone()
        }

        /// Parse the message data as JSON
        pub fn parse_json<T>(&self) -> Result<T>
        where
            T: for<'de> Deserialize<'de>,
        {
            serde_json::from_slice(&self.data)
                .map_err(|e| Error::serialization(format!("JSON parse error: {}", e)))
        }

        /// Get the message data as a string slice
        pub fn as_str(&self) -> Result<&str> {
            std::str::from_utf8(&self.data)
                .map_err(|e| Error::serialization(format!("Invalid UTF-8: {}", e)))
        }

        /// Create a message from a file (ASYNC - Non-blocking fallback!)
        ///
        /// This is the async version of `from_file` for the non-mmap fallback
        /// that uses `tokio::task::spawn_blocking` to avoid blocking the async runtime.
        pub async fn from_file_async(
            id: MessageId,
            path: &std::path::Path,
            offset: usize,
            length: Option<usize>,
        ) -> std::io::Result<Self> {
            let path = path.to_path_buf(); // Clone for move into spawn_blocking

            tokio::task::spawn_blocking(move || Self::from_file(id, &path, offset, length))
                .await
                .map_err(|join_err| {
                    std::io::Error::other(format!("Async file operation failed: {}", join_err))
                })?
        }
    }

    /// Fallback pool implementation
    #[derive(Debug)]
    pub struct MmapPool {
        cache: dashmap::DashMap<std::path::PathBuf, Bytes>,
        max_size: usize,
    }

    impl MmapPool {
        /// Create a new MmapPool with the specified maximum cache size
        pub fn new(max_size: usize) -> Self {
            Self {
                cache: dashmap::DashMap::new(),
                max_size,
            }
        }

        /// Get or create a cached file read
        pub fn get_or_create(&self, path: &Path) -> io::Result<Bytes> {
            if let Some(data) = self.cache.get(path) {
                return Ok(data.clone());
            }

            let data = fs::read(path)?;
            let bytes = Bytes::from(data);

            if self.cache.len() < self.max_size {
                self.cache.insert(path.to_path_buf(), bytes.clone());
            }

            Ok(bytes)
        }

        /// Clear all cached entries
        pub fn clear(&self) {
            self.cache.clear();
        }

        /// Get the current number of cached entries
        pub fn size(&self) -> usize {
            self.cache.len()
        }
    }

    /// Fallback batch implementation
    #[derive(Debug)]
    pub struct MmapBatch {
        data: Bytes,
        messages: Vec<(usize, usize)>,
        ids: Vec<Arc<MessageId>>,
    }

    impl MmapBatch {
        /// Create a batch from a JSONL file
        pub fn from_jsonl_file(path: &Path) -> io::Result<Self> {
            let data = fs::read(path)?;
            let mut messages = Vec::new();
            let mut ids = Vec::new();
            let mut offset = 0;

            for (idx, line) in data.split(|&b| b == b'\n').enumerate() {
                if !line.is_empty() {
                    messages.push((offset, line.len()));
                    ids.push(Arc::new(MessageId::Number(idx as i64)));
                }
                offset += line.len() + 1;
            }

            Ok(Self {
                data: Bytes::from(data),
                messages,
                ids,
            })
        }

        /// Get a message by index
        #[inline]
        pub fn get(&self, index: usize) -> Option<&[u8]> {
            self.messages
                .get(index)
                .map(|(offset, length)| &self.data[*offset..*offset + *length])
        }

        /// Iterate over all messages in the batch
        pub fn iter(&self) -> impl Iterator<Item = (&Arc<MessageId>, &[u8])> + '_ {
            self.ids
                .iter()
                .zip(self.messages.iter())
                .map(move |(id, (offset, length))| (id, &self.data[*offset..*offset + *length]))
        }

        /// Get the number of messages in the batch
        pub fn len(&self) -> usize {
            self.messages.len()
        }

        /// Check if the batch is empty
        pub fn is_empty(&self) -> bool {
            self.messages.is_empty()
        }

        /// Create a batch from a JSONL file (ASYNC - Non-blocking fallback!)
        ///
        /// This is the async version of `from_jsonl_file` for the non-mmap fallback
        /// that uses `tokio::task::spawn_blocking` to avoid blocking the async runtime.
        pub async fn from_jsonl_file_async(path: &std::path::Path) -> std::io::Result<Self> {
            let path = path.to_path_buf(); // Clone for move into spawn_blocking

            tokio::task::spawn_blocking(move || Self::from_jsonl_file(&path))
                .await
                .map_err(|join_err| {
                    std::io::Error::other(format!(
                        "Async JSONL batch operation failed: {}",
                        join_err
                    ))
                })?
        }
    }
}

impl From<String> for MessageId {
    fn from(s: String) -> Self {
        Self::String(Arc::from(s))
    }
}

impl From<&str> for MessageId {
    fn from(s: &str) -> Self {
        Self::String(Arc::from(s))
    }
}

impl From<i64> for MessageId {
    fn from(n: i64) -> Self {
        Self::Number(n)
    }
}

impl From<Uuid> for MessageId {
    fn from(u: Uuid) -> Self {
        Self::Uuid(u)
    }
}

impl fmt::Display for MessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::String(s) => write!(f, "{}", s),
            Self::Number(n) => write!(f, "{}", n),
            Self::Uuid(u) => write!(f, "{}", u),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zero_copy_message_creation() {
        let payload = Bytes::from(r#"{"test": "data"}"#);
        let msg = ZeroCopyMessage::from_bytes(MessageId::from("test-1"), payload.clone());

        assert_eq!(msg.payload, payload);
        assert_eq!(msg.metadata.size, payload.len());
    }

    #[test]
    fn test_lazy_json_parsing() {
        let payload = Bytes::from(r#"{"key": "value", "number": 42}"#);
        let mut msg = ZeroCopyMessage::from_bytes(MessageId::from("test-2"), payload);

        // Parse lazily
        let raw = msg.parse_json_lazy().unwrap();
        assert!(raw.get().contains("value"));

        // Check that lazy_json is now populated
        assert!(msg.lazy_json.is_some());
    }

    #[test]
    fn test_buffer_pool() {
        let pool = BufferPool::new(2, 1024);

        let buf1 = pool.acquire();
        let buf2 = pool.acquire();
        let buf3 = pool.acquire(); // Should create new

        assert_eq!(buf1.capacity(), 1024);
        assert_eq!(buf2.capacity(), 1024);
        assert_eq!(buf3.capacity(), 1024);

        pool.release(buf1);
        let buf4 = pool.acquire(); // Should reuse
        assert_eq!(buf4.capacity(), 1024);
    }

    #[test]
    fn test_message_batch() {
        let mut batch = MessageBatch::new(10);

        batch.add(MessageId::from("msg1"), Bytes::from("data1"));
        batch.add(MessageId::from("msg2"), Bytes::from("data2"));
        batch.add(MessageId::from("msg3"), Bytes::from("data3"));

        assert_eq!(batch.messages.len(), 3);

        let msg1 = batch.get(0).unwrap();
        assert_eq!(msg1, Bytes::from("data1"));

        let msg2 = batch.get(1).unwrap();
        assert_eq!(msg2, Bytes::from("data2"));

        // Iterate without copying
        let mut count = 0;
        for (_id, payload) in batch.iter() {
            count += 1;
            assert!(!payload.is_empty());
        }
        assert_eq!(count, 3);
    }

    #[test]
    fn test_cheap_clone() {
        let msg = ZeroCopyMessage::from_bytes(MessageId::from("test"), Bytes::from("data"));

        let cloned = msg.cheap_clone();

        // Should share the same Arc pointers
        assert!(Arc::ptr_eq(&msg.id, &cloned.id));
        assert_eq!(msg.payload, cloned.payload);
    }

    #[test]
    fn test_mmap_message() {
        use std::io::Write;

        // Create a test file
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("test_mmap.json");
        let mut file = std::fs::File::create(&test_file).unwrap();
        let test_data = r#"{"test": "data", "value": 42}"#;
        file.write_all(test_data.as_bytes()).unwrap();
        file.sync_all().unwrap();
        drop(file);

        // Test memory-mapped message
        let msg = mmap::MmapMessage::from_file(MessageId::from("mmap-test"), &test_file, 0, None)
            .unwrap();

        assert_eq!(msg.data(), test_data.as_bytes());
        assert_eq!(msg.as_str().unwrap(), test_data);

        // Test JSON parsing
        let value: serde_json::Value = msg.parse_json().unwrap();
        assert_eq!(value["test"], "data");
        assert_eq!(value["value"], 42);

        // Clean up
        std::fs::remove_file(test_file).unwrap();
    }

    #[test]
    fn test_mmap_batch() {
        use std::io::Write;

        // Create a test JSONL file
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("test_batch.jsonl");
        let mut file = std::fs::File::create(&test_file).unwrap();
        writeln!(file, r#"{{"id": 1, "name": "first"}}"#).unwrap();
        writeln!(file, r#"{{"id": 2, "name": "second"}}"#).unwrap();
        writeln!(file, r#"{{"id": 3, "name": "third"}}"#).unwrap();
        file.sync_all().unwrap();
        drop(file);

        // Test batch processing
        let batch = mmap::MmapBatch::from_jsonl_file(&test_file).unwrap();

        assert_eq!(batch.len(), 3);
        assert!(!batch.is_empty());

        // Test individual access
        let msg1 = batch.get(0).unwrap();
        let value: serde_json::Value = serde_json::from_slice(msg1).unwrap();
        assert_eq!(value["id"], 1);
        assert_eq!(value["name"], "first");

        // Test iteration
        let mut count = 0;
        for (_id, data) in batch.iter() {
            let value: serde_json::Value = serde_json::from_slice(data).unwrap();
            assert!(value["id"].is_number());
            assert!(value["name"].is_string());
            count += 1;
        }
        assert_eq!(count, 3);

        // Clean up
        std::fs::remove_file(test_file).unwrap();
    }

    #[test]
    fn test_mmap_pool() {
        use std::io::Write;

        // Create test files
        let temp_dir = std::env::temp_dir();
        let test_file1 = temp_dir.join("pool_test1.json");
        let test_file2 = temp_dir.join("pool_test2.json");

        let mut file1 = std::fs::File::create(&test_file1).unwrap();
        file1.write_all(b"test1").unwrap();
        file1.sync_all().unwrap();

        let mut file2 = std::fs::File::create(&test_file2).unwrap();
        file2.write_all(b"test2").unwrap();
        file2.sync_all().unwrap();

        // Test pool
        let pool = mmap::MmapPool::new(10);

        assert_eq!(pool.size(), 0);

        let _data1 = pool.get_or_create(&test_file1).unwrap();
        assert_eq!(pool.size(), 1);

        let _data2 = pool.get_or_create(&test_file2).unwrap();
        assert_eq!(pool.size(), 2);

        // Getting again should use cache
        let _data1_again = pool.get_or_create(&test_file1).unwrap();
        assert_eq!(pool.size(), 2); // Still 2, used cache

        pool.clear();
        assert_eq!(pool.size(), 0);

        // Clean up
        std::fs::remove_file(test_file1).unwrap();
        std::fs::remove_file(test_file2).unwrap();
    }

    // ============================================================================
    // Async Memory Mapping Tests - Production-Grade TDD
    // ============================================================================

    #[cfg(feature = "mmap")]
    mod async_mmap_tests {
        use super::MessageId;
        use super::mmap::*;
        use std::io::Write;
        use std::path::Path;

        #[tokio::test]
        async fn test_mmap_message_from_file_async_performance() {
            // Test that from_file_async doesn't block the async runtime

            let temp_dir = std::env::temp_dir();
            let test_file = temp_dir.join("async_mmap_test.json");

            // Create test file
            {
                let mut file = std::fs::File::create(&test_file).unwrap();
                let test_data = r#"{"test": "async_data", "large_field": "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua."}"#;
                file.write_all(test_data.as_bytes()).unwrap();
                file.sync_all().unwrap();
            }

            // Test concurrent async calls don't block each other
            let handles = (0..3)
                .map(|i| {
                    let test_file = test_file.clone();
                    tokio::spawn(async move {
                        let start_time = std::time::Instant::now();

                        // This will FAIL initially because we need to implement from_file_async
                        let result = MmapMessage::from_file_async(
                            MessageId::from(format!("async-test-{}", i)),
                            &test_file,
                            0,
                            None,
                        )
                        .await;

                        let duration = start_time.elapsed();

                        // Should complete quickly without blocking other async tasks
                        assert!(
                            duration.as_millis() < 100,
                            "Async mmap took {}ms - should be <100ms",
                            duration.as_millis()
                        );

                        (i, result)
                    })
                })
                .collect::<Vec<_>>();

            let start_time = std::time::Instant::now();
            let results = futures::future::join_all(handles).await;
            let total_duration = start_time.elapsed();

            // All concurrent operations should complete quickly
            assert!(
                total_duration.as_millis() < 200,
                "Concurrent async mmap operations took {}ms - should be <200ms",
                total_duration.as_millis()
            );

            // All should succeed and return valid messages
            for result in results {
                let (i, mmap_result) = result.unwrap();
                let mmap_msg = mmap_result.unwrap();
                assert_eq!(*mmap_msg.id, MessageId::from(format!("async-test-{}", i)));
                assert!(!mmap_msg.data().is_empty());
            }

            // Clean up
            std::fs::remove_file(test_file).unwrap();
        }

        #[tokio::test]
        async fn test_mmap_batch_from_jsonl_file_async_concurrency() {
            let temp_dir = std::env::temp_dir();
            let test_file = temp_dir.join("async_batch_test.jsonl");

            // Create JSONL test file
            {
                let mut file = std::fs::File::create(&test_file).unwrap();
                writeln!(file, r#"{{"id": "msg1", "data": "test1"}}"#).unwrap();
                writeln!(file, r#"{{"id": "msg2", "data": "test2"}}"#).unwrap();
                writeln!(file, r#"{{"id": "msg3", "data": "test3"}}"#).unwrap();
                file.sync_all().unwrap();
            }

            // Test that async version doesn't block concurrent operations
            let handles = (0..5)
                .map(|_| {
                    let test_file = test_file.clone();
                    tokio::spawn(async move {
                        let start_time = std::time::Instant::now();

                        // This will FAIL initially - need to implement from_jsonl_file_async
                        let result = MmapBatch::from_jsonl_file_async(&test_file).await;

                        let duration = start_time.elapsed();
                        assert!(
                            duration.as_millis() < 150,
                            "Async batch processing took {}ms - should be <150ms",
                            duration.as_millis()
                        );

                        result
                    })
                })
                .collect::<Vec<_>>();

            let results = futures::future::join_all(handles).await;

            // All should succeed
            for result in results {
                let batch = result.unwrap().unwrap();
                assert_eq!(batch.len(), 3);
            }

            std::fs::remove_file(test_file).unwrap();
        }

        #[tokio::test]
        async fn test_async_mmap_error_handling() {
            // Test async error handling for non-existent files
            let non_existent = Path::new("/tmp/does_not_exist_async.json");

            let result = MmapMessage::from_file_async(
                MessageId::String("error-test".to_string().into()),
                non_existent,
                0,
                None,
            )
            .await;

            assert!(result.is_err());

            // Error should be descriptive, not generic blocking I/O error
            let error_msg = format!("{}", result.unwrap_err());
            assert!(
                error_msg.contains("No such file") || error_msg.contains("not found"),
                "Error should be descriptive: {}",
                error_msg
            );
        }

        #[tokio::test]
        async fn test_async_mmap_maintains_functionality() {
            // Verify async versions provide same functionality as sync versions
            let temp_dir = std::env::temp_dir();
            let test_file = temp_dir.join("functionality_test.json");

            let test_data = r#"{"test": "functionality", "value": 42}"#;
            std::fs::write(&test_file, test_data).unwrap();

            // Test both sync and async versions
            let sync_result = MmapMessage::from_file(
                MessageId::String("sync".to_string().into()),
                &test_file,
                0,
                None,
            )
            .unwrap();

            let async_result = MmapMessage::from_file_async(
                MessageId::String("async".to_string().into()),
                &test_file,
                0,
                None,
            )
            .await
            .unwrap();

            // Same data should be accessible
            assert_eq!(sync_result.data(), async_result.data());
            assert_eq!(sync_result.data(), test_data.as_bytes());

            std::fs::remove_file(test_file).unwrap();
        }
    }
}
