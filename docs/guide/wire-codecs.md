# Wire Codecs

TurboMCP v3 introduces `turbomcp-wire`, a wire format codec abstraction layer for pluggable serialization.

## Overview

The wire codec layer provides:

- **JSON Codec** - Standard serde_json implementation (default)
- **SIMD JSON** - High-performance SIMD-accelerated parsing
- **MessagePack** - Compact binary format for internal use
- **Streaming Decoder** - Newline-delimited JSON streams
- **`no_std` Compatible** - Works in embedded and WASM environments

It is a standalone utility for code that encodes MCP messages itself. The
TurboMCP server and client transports speak JSON as MCP requires and do not
take a codec. `turbomcp-protocol` can depend on it through its `wire`,
`wire-simd`, and `wire-msgpack` features.

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
use turbomcp_wire::{Codec, JsonCodec};

let my_data = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "ping"});

let codec = JsonCodec::new(); // or JsonCodec::pretty() for indented output
let json_bytes = codec.encode(&my_data)?;
let parsed: serde_json::Value = codec.decode(&json_bytes)?;
```

### SimdJsonCodec

SIMD-accelerated JSON parsing using `sonic-rs`:

```toml
[dependencies]
turbomcp-wire = { version = "3.5.0", features = ["simd"] }
```

```rust
use turbomcp_wire::{Codec, SimdJsonCodec};

let json_bytes = br#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;

let codec = SimdJsonCodec::new();
// Faster parsing on supported platforms
let parsed: serde_json::Value = codec.decode(json_bytes)?;
```

### MsgPackCodec

Compact binary MessagePack format:

```toml
[dependencies]
turbomcp-wire = { version = "3.5.0", features = ["msgpack"] }
```

```rust
use turbomcp_wire::{Codec, MsgPackCodec};

let my_data = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "ping"});

let codec = MsgPackCodec::new();
let binary = codec.encode(&my_data)?; // Smaller than JSON
let parsed: serde_json::Value = codec.decode(&binary)?;
```

## Streaming Decoder

For newline-delimited JSON arriving in arbitrary chunks (a line-based
transport, or the `data:` payloads of an event stream once the SSE framing is
removed):

```rust
use turbomcp_wire::StreamingJsonDecoder;

let mut decoder = StreamingJsonDecoder::new();

// Feed data as it arrives; a message may be split across chunks
decoder.feed(br#"{"jsonrpc":"2.0","id":1,"#);
decoder.feed(b"\"result\":{}}\n");

// Try to decode complete messages
while let Some(msg) = decoder.try_decode::<serde_json::Value>()? {
    println!("{msg}");
}
```

The buffer is capped (1 MiB by default; `with_max_size` changes it): a line that
exceeds it is discarded and the next `try_decode` returns an error.

### Stream Integration Example

```rust
use futures::{Stream, StreamExt};
use turbomcp_protocol::jsonrpc::JsonRpcMessage;
use turbomcp_wire::{CodecResult, StreamingJsonDecoder};

async fn process_stream(mut stream: impl Stream<Item = Vec<u8>> + Unpin) -> CodecResult<()> {
    let mut decoder = StreamingJsonDecoder::new();

    while let Some(chunk) = stream.next().await {
        decoder.feed(&chunk);

        while let Some(msg) = decoder.try_decode::<JsonRpcMessage>()? {
            match msg {
                JsonRpcMessage::Request(req) => println!("request {}", req.method),
                JsonRpcMessage::Response(res) => println!("response, success: {}", res.is_success()),
                JsonRpcMessage::Notification(notif) => println!("notification {}", notif.method),
            }
        }
    }
    Ok(())
}
```

## Dynamic Codec Selection

Use `AnyCodec` for runtime codec selection:

```rust
use turbomcp_wire::AnyCodec;

let my_data = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "ping"});

// Create codec by name; None if unknown or its feature is off
let codec = AnyCodec::from_name("json").expect("json is always available");
// Or: AnyCodec::from_name("simd") (feature `simd`)
// Or: AnyCodec::from_name("msgpack") (feature `msgpack`)

let bytes = codec.encode(&my_data)?;

// List available codecs
println!("Available: {:?}", AnyCodec::available_names());
// With every feature: ["json", "simd-json", "msgpack"]
```

### Content-Type Negotiation

```rust
use turbomcp_wire::AnyCodec;

fn get_codec_for_content_type(content_type: &str) -> AnyCodec {
    let name = match content_type {
        "application/msgpack" => "msgpack",
        _ => "json",
    };
    AnyCodec::from_name(name)
        .or_else(|| AnyCodec::from_name("json"))
        .expect("json is always available")
}
```

## The Codec Trait

Implement custom codecs by implementing the `Codec` trait:

```rust
use serde::{Serialize, de::DeserializeOwned};
use turbomcp_wire::{Codec, CodecError, CodecResult};

/// JSON with a trailing newline, for line-based transports.
pub struct NdjsonCodec;

impl Codec for NdjsonCodec {
    fn name(&self) -> &'static str {
        "ndjson"
    }

    fn content_type(&self) -> &'static str {
        "application/x-ndjson"
    }

    fn encode<T: Serialize>(&self, value: &T) -> CodecResult<Vec<u8>> {
        let mut bytes = serde_json::to_vec(value).map_err(|e| CodecError::encode(e.to_string()))?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    fn decode<T: DeserializeOwned>(&self, bytes: &[u8]) -> CodecResult<T> {
        serde_json::from_slice(bytes.trim_ascii_end()).map_err(|e| CodecError::decode(e.to_string()))
    }
}
```

## Feature Flags

| Feature | Description | Default |
|---------|-------------|---------|
| `std` | Standard library support | Yes |
| `json` | Compatibility alias; JSON is always available | No |
| `simd` | SIMD-accelerated JSON (sonic-rs) | No |
| `msgpack` | MessagePack binary format | No |
| `full` | All features | No |

## Performance Comparison

Relative speed depends on payload shape and CPU, so measure with your own
messages. `SimdJsonCodec` mainly speeds up decoding, and `MsgPackCodec` produces
smaller payloads than JSON.

## no_std Support

Wire codecs work in `no_std` environments:

```toml
[dependencies]
turbomcp-wire = { version = "3.5.0", default-features = false }
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

The built-in transports do not use wire codecs: MCP's STDIO, Streamable HTTP,
and WebSocket transports carry JSON, and the server and client encode it with
`serde_json` (and SIMD JSON inside `turbomcp-protocol`). Use a codec for
channels you control, such as messages between your own services.

### gRPC Transport

gRPC uses Protocol Buffers natively, not wire codecs.

## Error Handling

`CodecError` is a struct with a `message` (and an optional `source`), and it
converts into `McpError`:

```rust
use turbomcp_wire::{Codec, JsonCodec, McpError};

let codec = JsonCodec::new();
let invalid_bytes = b"{not json";

match codec.decode::<serde_json::Value>(invalid_bytes) {
    Ok(value) => println!("{value}"),
    Err(e) => {
        eprintln!("Codec error: {}", e.message);
        let mcp: McpError = e.into();
    }
}
```

## Best Practices

### 1. Use SIMD for High-Throughput Servers

```rust
use turbomcp_wire::SimdJsonCodec;

// Where you decode many messages yourself
let codec = SimdJsonCodec::new();
```

### 2. Use MessagePack for Internal Communication

```rust
use turbomcp_wire::MsgPackCodec;

// Between your own services (not MCP clients, which expect JSON)
let codec = MsgPackCodec::new();
```

### 3. Use Streaming Decoder for SSE

```rust
use turbomcp_wire::StreamingJsonDecoder;

// For newline-delimited streams: handles partial messages correctly
let mut decoder = StreamingJsonDecoder::new();
```

### 4. Match Content-Type Headers

```rust
use turbomcp_wire::AnyCodec;

fn encode_for(content_type: &str, value: &serde_json::Value) -> Vec<u8> {
    let codec = match content_type {
        "application/msgpack" => AnyCodec::from_name("msgpack"),
        _ => None,
    }
    .unwrap_or_else(|| AnyCodec::from_name("json").expect("json is always available"));
    codec.encode(value).unwrap_or_default()
}
```

## Next Steps

- **[Transports](transports.md)** - Transport layer details
- **[Tower Middleware](tower-middleware.md)** - Composable middleware
- **[Performance](../deployment/production.md)** - Production optimization
- **[API Reference](../api/wire.md)** - Full Wire API
