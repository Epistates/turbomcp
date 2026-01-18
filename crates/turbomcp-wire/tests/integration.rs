use turbomcp_wire::{AnyCodec, Codec, JsonCodec};
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct MockMessage {
    id: u32,
    method: String,
}

#[test]
fn test_any_codec_dispatch() {
    let codec = AnyCodec::from_name("json").expect("Codec not found");
    let msg = MockMessage { id: 1, method: "test".into() };
    
    let encoded = codec.encode(&msg).expect("Encode failed");
    let decoded: MockMessage = codec.decode(&encoded).expect("Decode failed");
    
    assert_eq!(msg, decoded);
    assert_eq!(codec.content_type(), "application/json");
}

#[test]
fn test_json_codec_explicit() {
    let codec = JsonCodec::new();
    let msg = MockMessage { id: 2, method: "ping".into() };
    
    let encoded = codec.encode(&msg).unwrap();
    let json_str = String::from_utf8(encoded).unwrap();
    assert!(json_str.contains("\"id\":2"));
}

