# Wire Codecs API Reference

The `turbomcp-wire` crate provides wire format codec abstraction for pluggable serialization in TurboMCP v3.

## Overview

Wire codecs handle encoding and decoding of MCP protocol messages. The crate supports multiple serialization formats with a common trait interface.

## Installation

```toml
[dependencies]
# Default (JSON codec only)
turbomcp-wire = "3.5.0"

# With SIMD acceleration
turbomcp-wire = { version = "3.5.0", features = ["simd"] }

# With MessagePack
turbomcp-wire = { version = "3.5.0", features = ["msgpack"] }

# All codecs
turbomcp-wire = { version = "3.5.0", features = ["full"] }
```

## Feature Flags

| Feature | Description | Default |
|---------|-------------|---------|
| `std` | Standard library support | Yes |
| `json` | Compatibility alias; JSON codec is always available | N/A |
| `simd` | SIMD-accelerated JSON (sonic-rs) | No |
| `msgpack` | MessagePack binary format (rmp-serde) | No |
| `full` | All features | No |

## The Codec Trait

The trait as defined in `turbomcp-wire` (doc comments trimmed):

```rust,ignore
pub trait Codec: Send + Sync {
    /// Encode a value to bytes
    fn encode<T: Serialize>(&self, value: &T) -> CodecResult<Vec<u8>>;

    /// Decode bytes to a value
    fn decode<T: DeserializeOwned>(&self, bytes: &[u8]) -> CodecResult<T>;

    /// Content type for this codec (e.g. "application/json")
    fn content_type(&self) -> &'static str;

    /// Whether this codec supports streaming (default: false)
    fn supports_streaming(&self) -> bool {
        false
    }

    /// Codec name for debugging
    fn name(&self) -> &'static str;
}

pub type CodecResult<T> = Result<T, CodecError>;
```

Because `encode` and `decode` are generic, `Codec` is not object-safe; use
[`AnyCodec`](#anycodec) to choose a codec at runtime.

## JsonCodec

Standard JSON codec using `serde_json`.

```rust
use serde::{Deserialize, Serialize};
use turbomcp_wire::{Codec, JsonCodec};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct MyData {
    field: String,
}

fn main() -> Result<(), turbomcp_wire::CodecError> {
    let codec = JsonCodec::new();

    // Encode
    let data = MyData { field: "value".to_string() };
    let bytes = codec.encode(&data)?;

    // Decode
    let parsed: MyData = codec.decode(&bytes)?;
    assert_eq!(parsed, data);

    // Codec name and content type
    assert_eq!(codec.name(), "json");
    assert_eq!(codec.content_type(), "application/json");
    Ok(())
}
```

### Pretty Printing

```rust
use turbomcp_wire::{Codec, JsonCodec};

let codec = JsonCodec::pretty();
let bytes = codec.encode(&serde_json::json!({"field": "value"})).unwrap();
// Output is formatted with indentation
```

## SimdJsonCodec

SIMD-accelerated JSON using `sonic-rs` (feature `simd`). Same API and output as `JsonCodec`.

```rust
use turbomcp_wire::{Codec, SimdJsonCodec};

let codec = SimdJsonCodec::new();

// Same API as JsonCodec
let bytes = codec.encode(&serde_json::json!({"id": 1})).unwrap();
let parsed: serde_json::Value = codec.decode(&bytes).unwrap();

assert_eq!(codec.name(), "simd-json");
```

### Platform Support

`sonic-rs` detects SIMD support at runtime and has its own scalar fallback;
the codec adds no fallback to `serde_json` on top of that.

## MsgPackCodec

Compact binary MessagePack format (feature `msgpack`). Smaller payloads than JSON.
Values are encoded with named fields (`rmp_serde::to_vec_named`).

```rust
use turbomcp_wire::{Codec, MsgPackCodec};

let codec = MsgPackCodec::new();

let bytes = codec.encode(&serde_json::json!({"id": 1, "method": "ping"})).unwrap();
let parsed: serde_json::Value = codec.decode(&bytes).unwrap();

assert_eq!(codec.name(), "msgpack");
```

## AnyCodec

Dynamic codec selection at runtime. `AnyCodec` is an enum over the codecs the
enabled features provide, with the same `encode` / `decode` / `name` /
`content_type` methods.

```rust
use turbomcp_wire::AnyCodec;

// Create by name: "json", "simd" or "simd-json", "msgpack".
// None if the name is unknown or its feature is off.
let codec = AnyCodec::from_name("json").expect("json is always available");
let simd = AnyCodec::from_name("simd");
let msgpack = AnyCodec::from_name("msgpack");

// List available codecs
let names = AnyCodec::available_names();
// ["json", "simd-json", "msgpack"] (depending on features)

// Use like any codec
let bytes = codec.encode(&serde_json::json!({"id": 1})).unwrap();
let parsed: serde_json::Value = codec.decode(&bytes).unwrap();
```

### Constructing Variants

There are no per-codec convenience constructors; build a variant directly:

```rust
use turbomcp_wire::{AnyCodec, JsonCodec};

let json = AnyCodec::Json(JsonCodec::new());
```

## StreamingJsonDecoder

Incremental decoder for newline-delimited JSON streams (NDJSON).

```rust
use serde::Deserialize;
use turbomcp_wire::StreamingJsonDecoder;

#[derive(Debug, Deserialize)]
struct Message {
    id: u32,
}

fn main() -> Result<(), turbomcp_wire::CodecError> {
    let mut decoder = StreamingJsonDecoder::new();

    // Feed data as it arrives
    decoder.feed(b"{ \"id\": 1 }\n");
    decoder.feed(b"{ \"id\": 2 }\n{ \"id\":");
    decoder.feed(b" 3 }\n");

    // Decode complete messages
    while let Some(msg) = decoder.try_decode::<Message>()? {
        println!("Received: {:?}", msg);
    }
    // Output: Message { id: 1 }, Message { id: 2 }, Message { id: 3 }
    Ok(())
}
```

### Methods

Signatures only:

```rust,ignore
impl StreamingJsonDecoder {
    /// Create a new decoder (buffer limit: 1 MiB)
    pub fn new() -> Self;

    /// Pre-allocate `capacity` bytes (clamped to the default limit)
    pub fn with_capacity(capacity: usize) -> Self;

    /// Set the buffer limit (capped at 10 MiB)
    pub fn with_max_size(max_size: usize) -> Self;

    /// Feed bytes into the decoder buffer
    pub fn feed(&mut self, data: &[u8]);

    /// Try to decode the next complete message
    pub fn try_decode<T: DeserializeOwned>(&mut self) -> CodecResult<Option<T>>;

    /// Clear the internal buffer
    pub fn clear(&mut self);

    /// Check if buffer is empty
    pub fn is_empty(&self) -> bool;

    /// Get buffer length
    pub fn len(&self) -> usize;

    /// Get the buffer limit
    pub fn max_buffer_size(&self) -> usize;
}
```

When fed data exceeds the limit, the unfinished message is dropped and the next
`try_decode` returns an error once, so the loss is visible; decoding resumes at
the next newline.

### Streaming Integration Example

```rust
use bytes::Bytes;
use futures::{Stream, StreamExt};
use turbomcp_wire::{CodecError, StreamingJsonDecoder};

async fn process(mut stream: impl Stream<Item = Bytes> + Unpin) -> Result<(), CodecError> {
    let mut decoder = StreamingJsonDecoder::new();

    while let Some(chunk) = stream.next().await {
        decoder.feed(&chunk);

        while let Some(msg) = decoder.try_decode::<serde_json::Value>()? {
            println!("message: {msg}");
        }
    }
    Ok(())
}
```

## CodecError

Error type for codec operations: a struct with a message and optional source,
not an enum.

```rust,ignore
pub struct CodecError {
    /// Error message ("encode: …" / "decode: …")
    pub message: String,
    /// Optional source location
    pub source: Option<String>,
}
```

It converts into `McpError` (as a parse error) with `?` or `.into()`.

### Error Handling

```rust
use turbomcp_wire::{Codec, JsonCodec};

let codec = JsonCodec::new();

match codec.decode::<serde_json::Value>(b"{not json") {
    Ok(value) => println!("Decoded: {:?}", value),
    Err(e) => eprintln!("Parse error: {}", e.message),
}
```

## Custom Codec Implementation

Implement your own codec. This one wraps `serde_json` with a different content type:

```rust
use serde::{Serialize, de::DeserializeOwned};
use turbomcp_wire::{Codec, CodecError, CodecResult};

pub struct NdJsonCodec;

impl Codec for NdJsonCodec {
    fn encode<T: Serialize>(&self, value: &T) -> CodecResult<Vec<u8>> {
        let mut bytes = serde_json::to_vec(value).map_err(|e| CodecError::encode(e.to_string()))?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    fn decode<T: DeserializeOwned>(&self, bytes: &[u8]) -> CodecResult<T> {
        serde_json::from_slice(bytes).map_err(|e| CodecError::decode(e.to_string()))
    }

    fn content_type(&self) -> &'static str {
        "application/x-ndjson"
    }

    fn name(&self) -> &'static str {
        "ndjson"
    }
}
```

## Codec Selection Guide

| Use Case | Recommended Codec |
|----------|-------------------|
| General use | `JsonCodec` |
| High-throughput servers | `SimdJsonCodec` |
| Internal microservices | `MsgPackCodec` |
| Browser clients | `JsonCodec` |
| Bandwidth-constrained | `MsgPackCodec` |
| Debugging/logging | `JsonCodec::pretty()` |

MCP itself is JSON on every standard transport; MessagePack is for links where
you control both ends.

## Performance Benchmarks

Measured on Apple M2, 1KB payload:

| Codec | Encode | Decode | Size |
|-------|--------|--------|------|
| JsonCodec | 1.2 μs | 2.1 μs | 1024 B |
| SimdJsonCodec | 0.8 μs | 0.7 μs | 1024 B |
| MsgPackCodec | 0.5 μs | 0.4 μs | 680 B |

Run benchmarks:

```bash
cargo bench -p turbomcp-wire --features full
```

## no_std Support

Wire codecs work in `no_std` environments (`default-features = false`). This
fragment is the root of a `no_std` library crate:

```rust,ignore
#![no_std]
extern crate alloc;

use alloc::vec::Vec;
use turbomcp_wire::{Codec, JsonCodec};

fn encode<T: serde::Serialize>(data: &T) -> Vec<u8> {
    let codec = JsonCodec::new();
    codec.encode(data).unwrap()
}
```

## Thread Safety

All codecs are `Send + Sync` and can be shared across threads:

```rust
use std::sync::Arc;
use turbomcp_wire::{Codec, JsonCodec};

let codec = Arc::new(JsonCodec::new());

// Use from multiple threads
let codec_clone = codec.clone();
std::thread::spawn(move || {
    let bytes = codec_clone.encode(&serde_json::json!({"id": 1})).unwrap();
});
```

## Next Steps

- **[Wire Codecs Guide](../guide/wire-codecs.md)** - Usage patterns
- **[Transports Guide](../guide/transports.md)** - Transport integration
- **[Core Types](core.md)** - MCP type definitions
