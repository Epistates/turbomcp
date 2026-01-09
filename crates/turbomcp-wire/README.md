# turbomcp-wire

Wire format codec abstraction for TurboMCP - JSON-RPC encoding/decoding with pluggable serialization.

## Overview

This crate provides the wire format layer for MCP protocol communication. It abstracts over different serialization formats while maintaining MCP protocol compliance.

## Features

- **JSON Codec** - Standard serde_json implementation (default)
- **SIMD JSON** - High-performance SIMD-accelerated parsing (optional)
- **MessagePack** - Compact binary format for internal use (optional)
- **Streaming Decoder** - Newline-delimited JSON for SSE transports
- **`no_std` Compatible** - Works in embedded and WASM environments

## Usage

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

## Streaming Decoder

For HTTP/SSE transports with newline-delimited JSON:

```rust
use turbomcp_wire::StreamingJsonDecoder;

let mut decoder = StreamingJsonDecoder::new();

// Feed data as it arrives
decoder.feed(data_chunk);

// Try to decode complete messages
while let Some(msg) = decoder.try_decode::<MyMessage>()? {
    handle_message(msg);
}
```

## Feature Flags

| Feature | Description |
|---------|-------------|
| `std` | Standard library support (default) |
| `json` | JSON codec (default) |
| `simd` | SIMD-accelerated JSON (sonic-rs) |
| `msgpack` | MessagePack binary format |
| `full` | All features |

## Dynamic Codec Selection

Create codecs dynamically by name using `AnyCodec`:

```rust
use turbomcp_wire::AnyCodec;

let codec = AnyCodec::from_name("json").unwrap();
let bytes = codec.encode(&my_data).unwrap();
println!("Available codecs: {:?}", AnyCodec::available_names());
```

## Performance

With `simd` feature enabled, JSON parsing can be 2-4x faster on supported platforms:

```bash
cargo bench --features simd
```

## License

MIT
