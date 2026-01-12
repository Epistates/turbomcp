# Wire Codecs

TurboMCP v3 introduces `turbomcp-wire`, a wire format codec abstraction layer for pluggable serialization.

## Overview

The wire codec layer provides:

- **JSON Codec** - Standard serde_json implementation (default)
- **SIMD JSON** - High-performance SIMD-accelerated parsing
- **MessagePack** - Compact binary format for internal use
- **Streaming Decoder** - Newline-delimited JSON for SSE transports
- **`no_std` Compatible** - Works in embedded and WASM environments

## Basic Usage

```rust
use turbomcp_wire::{Codec, JsonCodec};
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
struct Request {
    jsonrpc: String,
    id: u32,
    method: String,
}

let codec = JsonCodec::new();

// Encode
let request = Request {
    jsonrpc: "2.0".into(),
    id: 1,
    method: "initialize".into(),
};
let bytes = codec.encode(&request).unwrap();

// Decode
let decoded: Request = codec.decode(&bytes).unwrap();
```

## Available Codecs

### JsonCodec (Default)

Standard JSON codec using `serde_json`:

```rust
use turbomcp_wire::JsonCodec;

let codec = JsonCodec::new();
let json_bytes = codec.encode(&my_data)?;
let parsed: MyType = codec.decode(&json_bytes)?;
```

### SimdJsonCodec

SIMD-accelerated JSON parsing using `sonic-rs`:

```toml
[dependencies]
turbomcp-wire = { version = "3.0", features = ["simd"] }
```

```rust
use turbomcp_wire::SimdJsonCodec;

let codec = SimdJsonCodec::new();
// 2-4x faster parsing on supported platforms
let parsed: MyType = codec.decode(&json_bytes)?;
```

### MsgPackCodec

Compact binary MessagePack format:

```toml
[dependencies]
turbomcp-wire = { version = "3.0", features = ["msgpack"] }
```

```rust
use turbomcp_wire::MsgPackCodec;

let codec = MsgPackCodec::new();
let binary = codec.encode(&my_data)?;  // Smaller than JSON
let parsed: MyType = codec.decode(&binary)?;
```

## Streaming Decoder

For HTTP/SSE transports with newline-delimited JSON:

```rust
use turbomcp_wire::StreamingJsonDecoder;

let mut decoder = StreamingJsonDecoder::new();

// Feed data as it arrives (e.g., from SSE stream)
decoder.feed(data_chunk);

// Try to decode complete messages
while let Some(msg) = decoder.try_decode::<MyMessage>()? {
    handle_message(msg);
}
```

### SSE Integration Example

```rust
use turbomcp_wire::StreamingJsonDecoder;
use futures::StreamExt;

async fn process_sse_stream(mut stream: impl Stream<Item = Bytes>) {
    let mut decoder = StreamingJsonDecoder::new();

    while let Some(chunk) = stream.next().await {
        decoder.feed(&chunk);

        while let Some(msg) = decoder.try_decode::<McpMessage>()? {
            match msg {
                McpMessage::Request(req) => handle_request(req),
                McpMessage::Response(res) => handle_response(res),
                McpMessage::Notification(notif) => handle_notification(notif),
            }
        }
    }
}
```

## Dynamic Codec Selection

Use `AnyCodec` for runtime codec selection:

```rust
use turbomcp_wire::AnyCodec;

// Create codec by name
let codec = AnyCodec::from_name("json")?;
// Or: AnyCodec::from_name("simd")
// Or: AnyCodec::from_name("msgpack")

let bytes = codec.encode(&my_data)?;

// List available codecs
println!("Available: {:?}", AnyCodec::available_names());
// Output: ["json", "simd", "msgpack"]
```

### Content-Type Negotiation

```rust
use turbomcp_wire::AnyCodec;

fn get_codec_for_content_type(content_type: &str) -> AnyCodec {
    match content_type {
        "application/json" => AnyCodec::from_name("json").unwrap(),
        "application/x-simd-json" => AnyCodec::from_name("simd").unwrap(),
        "application/msgpack" => AnyCodec::from_name("msgpack").unwrap(),
        _ => AnyCodec::from_name("json").unwrap(),
    }
}
```

## The Codec Trait

Implement custom codecs by implementing the `Codec` trait:

```rust
use turbomcp_wire::{Codec, CodecError};
use serde::{Serialize, de::DeserializeOwned};

pub struct MyCodec;

impl Codec for MyCodec {
    fn name(&self) -> &'static str {
        "my-codec"
    }

    fn encode<T: Serialize>(&self, value: &T) -> Result<Vec<u8>, CodecError> {
        // Your encoding logic
        todo!()
    }

    fn decode<T: DeserializeOwned>(&self, bytes: &[u8]) -> Result<T, CodecError> {
        // Your decoding logic
        todo!()
    }
}
```

## Feature Flags

| Feature | Description | Default |
|---------|-------------|---------|
| `std` | Standard library support | Yes |
| `json` | JSON codec | Yes |
| `simd` | SIMD-accelerated JSON (sonic-rs) | No |
| `msgpack` | MessagePack binary format | No |
| `full` | All features | No |

## Performance Comparison

Benchmarks on Apple M2 with 1KB JSON payload:

| Codec | Encode | Decode |
|-------|--------|--------|
| JsonCodec | 1.2 μs | 2.1 μs |
| SimdJsonCodec | 0.8 μs | 0.7 μs |
| MsgPackCodec | 0.5 μs | 0.4 μs |

Enable SIMD for significant speedup:

```bash
cargo bench --features simd
```

## no_std Support

Wire codecs work in `no_std` environments:

```toml
[dependencies]
turbomcp-wire = { version = "3.0", default-features = false, features = ["json"] }
```

```rust
#![no_std]
extern crate alloc;

use turbomcp_wire::{Codec, JsonCodec};
use alloc::vec::Vec;

fn encode_message<T: serde::Serialize>(msg: &T) -> Vec<u8> {
    let codec = JsonCodec::new();
    codec.encode(msg).unwrap()
}
```

## Transport Integration

Wire codecs are automatically used by transports:

### HTTP Transport

```rust
use turbomcp_http::HttpTransportConfig;

let config = HttpTransportConfig::builder()
    .codec("simd")  // Use SIMD codec for HTTP
    .build();
```

### WebSocket Transport

```rust
use turbomcp_websocket::WebSocketConfig;

let config = WebSocketConfig::builder()
    .codec("msgpack")  // Use MessagePack for WebSocket
    .build();
```

### gRPC Transport

gRPC uses Protocol Buffers natively, not wire codecs. The wire codec layer is used for JSON-RPC over other transports.

## Error Handling

```rust
use turbomcp_wire::{Codec, JsonCodec, CodecError};

let codec = JsonCodec::new();

match codec.decode::<MyType>(invalid_bytes) {
    Ok(value) => handle_value(value),
    Err(CodecError::DeserializeError(msg)) => {
        eprintln!("Failed to parse: {}", msg);
    }
    Err(CodecError::SerializeError(msg)) => {
        eprintln!("Failed to serialize: {}", msg);
    }
    Err(e) => {
        eprintln!("Codec error: {}", e);
    }
}
```

## Best Practices

### 1. Use SIMD for High-Throughput Servers

```rust
// In production servers handling many requests
let codec = SimdJsonCodec::new();  // 2-4x faster
```

### 2. Use MessagePack for Internal Communication

```rust
// Between microservices (not client-facing)
let codec = MsgPackCodec::new();  // 30-50% smaller
```

### 3. Use Streaming Decoder for SSE

```rust
// For Server-Sent Events streams
let mut decoder = StreamingJsonDecoder::new();
// Handles partial messages correctly
```

### 4. Match Content-Type Headers

```rust
// HTTP server
fn handle_request(req: &Request) -> Response {
    let codec = match req.content_type() {
        "application/msgpack" => AnyCodec::msgpack(),
        _ => AnyCodec::json(),
    };
    // ...
}
```

## Next Steps

- **[Transports](transports.md)** - Transport layer details
- **[Tower Middleware](tower-middleware.md)** - Composable middleware
- **[Performance](../deployment/production.md)** - Production optimization
- **[API Reference](../api/wire.md)** - Full Wire API
