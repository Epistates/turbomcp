# Wire Codecs API Reference

The `turbomcp-wire` crate provides wire format codec abstraction for pluggable serialization in TurboMCP v3.

## Overview

Wire codecs handle encoding and decoding of MCP protocol messages. The crate supports multiple serialization formats with a common trait interface.

## Installation

```toml
[dependencies]
# Default (JSON codec only)
turbomcp-wire = "3.0"

# With SIMD acceleration
turbomcp-wire = { version = "3.0", features = ["simd"] }

# With MessagePack
turbomcp-wire = { version = "3.0", features = ["msgpack"] }

# All codecs
turbomcp-wire = { version = "3.0", features = ["full"] }
```

## Feature Flags

| Feature | Description | Default |
|---------|-------------|---------|
| `std` | Standard library support | Yes |
| `json` | JSON codec (serde_json) | Yes |
| `simd` | SIMD-accelerated JSON (sonic-rs) | No |
| `msgpack` | MessagePack binary format | No |
| `full` | All features | No |

## The Codec Trait

```rust
use turbomcp_wire::Codec;
use serde::{Serialize, de::DeserializeOwned};

pub trait Codec: Send + Sync {
    /// Codec name for identification
    fn name(&self) -> &'static str;

    /// Encode a value to bytes
    fn encode<T: Serialize>(&self, value: &T) -> Result<Vec<u8>, CodecError>;

    /// Decode bytes to a value
    fn decode<T: DeserializeOwned>(&self, bytes: &[u8]) -> Result<T, CodecError>;
}
```

## JsonCodec

Standard JSON codec using `serde_json`.

```rust
use turbomcp_wire::JsonCodec;

let codec = JsonCodec::new();

// Encode
let data = MyData { field: "value" };
let bytes = codec.encode(&data)?;

// Decode
let parsed: MyData = codec.decode(&bytes)?;

// Codec name
assert_eq!(codec.name(), "json");
```

### Pretty Printing

```rust
use turbomcp_wire::JsonCodec;

let codec = JsonCodec::new().pretty(true);
let bytes = codec.encode(&data)?;
// Output is formatted with indentation
```

## SimdJsonCodec

SIMD-accelerated JSON parsing using `sonic-rs`. 2-4x faster than standard JSON on supported platforms.

```rust
use turbomcp_wire::SimdJsonCodec;

let codec = SimdJsonCodec::new();

// Same API as JsonCodec
let bytes = codec.encode(&data)?;
let parsed: MyData = codec.decode(&bytes)?;

assert_eq!(codec.name(), "simd");
```

### Platform Support

- x86_64 with AVX2/SSE4.2
- aarch64 with NEON

Falls back to standard JSON on unsupported platforms.

## MsgPackCodec

Compact binary MessagePack format. Smaller payloads than JSON.

```rust
use turbomcp_wire::MsgPackCodec;

let codec = MsgPackCodec::new();

let bytes = codec.encode(&data)?;  // ~30-50% smaller than JSON
let parsed: MyData = codec.decode(&bytes)?;

assert_eq!(codec.name(), "msgpack");
```

## AnyCodec

Dynamic codec selection at runtime.

```rust
use turbomcp_wire::AnyCodec;

// Create by name
let codec = AnyCodec::from_name("json")?;
let codec = AnyCodec::from_name("simd")?;
let codec = AnyCodec::from_name("msgpack")?;

// List available codecs
let names = AnyCodec::available_names();
// ["json", "simd", "msgpack"] (depending on features)

// Use like any codec
let bytes = codec.encode(&data)?;
let parsed: MyData = codec.decode(&bytes)?;
```

### Convenience Constructors

```rust
use turbomcp_wire::AnyCodec;

let json = AnyCodec::json();
let simd = AnyCodec::simd();      // Requires "simd" feature
let msgpack = AnyCodec::msgpack(); // Requires "msgpack" feature
```

## StreamingJsonDecoder

Incremental decoder for newline-delimited JSON streams (NDJSON).

```rust
use turbomcp_wire::StreamingJsonDecoder;

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
```

### Methods

```rust
impl StreamingJsonDecoder {
    /// Create a new decoder
    pub fn new() -> Self;

    /// Feed bytes into the decoder buffer
    pub fn feed(&mut self, data: &[u8]);

    /// Try to decode the next complete message
    pub fn try_decode<T: DeserializeOwned>(&mut self) -> Result<Option<T>, CodecError>;

    /// Clear the internal buffer
    pub fn clear(&mut self);

    /// Check if buffer is empty
    pub fn is_empty(&self) -> bool;

    /// Get buffer length
    pub fn len(&self) -> usize;
}
```

### SSE Integration Example

```rust
use turbomcp_wire::StreamingJsonDecoder;
use futures::StreamExt;

async fn process_sse(mut stream: impl Stream<Item = Bytes>) {
    let mut decoder = StreamingJsonDecoder::new();

    while let Some(chunk) = stream.next().await {
        decoder.feed(&chunk);

        while let Some(msg) = decoder.try_decode::<McpMessage>()? {
            handle_message(msg).await;
        }
    }
}
```

## CodecError

Error type for codec operations.

```rust
use turbomcp_wire::CodecError;

pub enum CodecError {
    /// Serialization failed
    SerializeError(String),

    /// Deserialization failed
    DeserializeError(String),

    /// Unknown codec name
    UnknownCodec(String),

    /// IO error (with std feature)
    #[cfg(feature = "std")]
    IoError(std::io::Error),
}
```

### Error Handling

```rust
use turbomcp_wire::{JsonCodec, Codec, CodecError};

let codec = JsonCodec::new();

match codec.decode::<MyType>(invalid_bytes) {
    Ok(value) => println!("Decoded: {:?}", value),
    Err(CodecError::DeserializeError(msg)) => {
        eprintln!("Parse error: {}", msg);
    }
    Err(e) => {
        eprintln!("Other error: {}", e);
    }
}
```

## Custom Codec Implementation

Implement your own codec:

```rust
use turbomcp_wire::{Codec, CodecError};
use serde::{Serialize, de::DeserializeOwned};

pub struct MyCodec {
    // Custom configuration
}

impl Codec for MyCodec {
    fn name(&self) -> &'static str {
        "my-codec"
    }

    fn encode<T: Serialize>(&self, value: &T) -> Result<Vec<u8>, CodecError> {
        // Your encoding logic
        my_format::to_vec(value)
            .map_err(|e| CodecError::SerializeError(e.to_string()))
    }

    fn decode<T: DeserializeOwned>(&self, bytes: &[u8]) -> Result<T, CodecError> {
        // Your decoding logic
        my_format::from_slice(bytes)
            .map_err(|e| CodecError::DeserializeError(e.to_string()))
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
| Debugging/logging | `JsonCodec` (pretty) |

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

Wire codecs work in `no_std` environments:

```rust
#![no_std]
extern crate alloc;

use turbomcp_wire::{Codec, JsonCodec};
use alloc::vec::Vec;

fn encode<T: serde::Serialize>(data: &T) -> Vec<u8> {
    let codec = JsonCodec::new();
    codec.encode(data).unwrap()
}
```

## Thread Safety

All codecs are `Send + Sync` and can be shared across threads:

```rust
use std::sync::Arc;
use turbomcp_wire::JsonCodec;

let codec = Arc::new(JsonCodec::new());

// Use from multiple threads
let codec_clone = codec.clone();
std::thread::spawn(move || {
    let bytes = codec_clone.encode(&data)?;
});
```

## Next Steps

- **[Wire Codecs Guide](../guide/wire-codecs.md)** - Usage patterns
- **[Transports Guide](../guide/transports.md)** - Transport integration
- **[Core Types](core.md)** - MCP type definitions
