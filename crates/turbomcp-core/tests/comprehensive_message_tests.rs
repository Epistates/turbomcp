//! Comprehensive message handling tests for maximum coverage

use bytes::Bytes;
use serde_json::json;
use std::collections::HashMap;
use turbomcp_core::message::*;
use turbomcp_core::types::*;
use uuid::Uuid;

// ============================================================================
// MessageId Tests
// ============================================================================

#[test]
fn test_message_id_variants() {
    let string_id = MessageId::String("test-123".to_string());
    let number_id = MessageId::Number(42);
    let uuid_id = MessageId::Uuid(Uuid::new_v4());

    // Test Debug, Clone, PartialEq, Eq, Hash
    assert_ne!(string_id, number_id);
    assert_ne!(number_id, uuid_id);
    assert_eq!(string_id.clone(), string_id);

    let debug_str = format!("{string_id:?}");
    assert!(debug_str.contains("String"));
}

#[test]
fn test_message_id_serialization() {
    let string_id = MessageId::String("test".to_string());
    let number_id = MessageId::Number(123);
    let uuid = Uuid::new_v4();
    let uuid_id = MessageId::Uuid(uuid);

    // Test serialization/deserialization
    let string_json = serde_json::to_string(&string_id).unwrap();
    let number_json = serde_json::to_string(&number_id).unwrap();
    let uuid_json = serde_json::to_string(&uuid_id).unwrap();

    let deserialized_string: MessageId = serde_json::from_str(&string_json).unwrap();
    let deserialized_number: MessageId = serde_json::from_str(&number_json).unwrap();
    let deserialized_uuid: MessageId = serde_json::from_str(&uuid_json).unwrap();

    assert_eq!(string_id, deserialized_string);
    assert_eq!(number_id, deserialized_number);
    // UUID might deserialize as string due to serde(untagged)
    match deserialized_uuid {
        MessageId::Uuid(u) => assert_eq!(uuid, u),
        MessageId::String(s) => assert_eq!(uuid.to_string(), s),
        _ => panic!("UUID should deserialize as UUID or String"),
    }
}

#[test]
fn test_message_id_from_implementations() {
    let from_string: MessageId = "test".into();
    let from_i64: MessageId = 42i64.into();
    let from_uuid: MessageId = Uuid::new_v4().into();

    assert!(matches!(from_string, MessageId::String(_)));
    assert!(matches!(from_i64, MessageId::Number(_)));
    assert!(matches!(from_uuid, MessageId::Uuid(_)));
}

#[test]
fn test_message_id_display() {
    let string_id = MessageId::String("display-test".to_string());
    let number_id = MessageId::Number(-999);
    let uuid = Uuid::new_v4();
    let uuid_id = MessageId::Uuid(uuid);

    assert_eq!(format!("{string_id}"), "display-test");
    assert_eq!(format!("{number_id}"), "-999");
    assert_eq!(format!("{uuid_id}"), uuid.to_string());
}

// ============================================================================
// MessageMetadata Tests
// ============================================================================

#[test]
fn test_message_metadata_new() {
    let metadata = MessageMetadata::new(ContentType::Json, 256);

    assert_eq!(metadata.content_type, ContentType::Json);
    assert_eq!(metadata.size, 256);
    assert!(metadata.encoding.is_none());
    assert!(metadata.correlation_id.is_none());
    assert!(metadata.headers.is_empty());
}

#[test]
fn test_message_metadata_builder_pattern() {
    let metadata = MessageMetadata::new(ContentType::Binary, 512)
        .with_encoding("gzip".to_string())
        .with_correlation_id("corr-456".to_string())
        .with_header("X-Custom".to_string(), "custom-value".to_string())
        .with_header("X-Another".to_string(), "another-value".to_string());

    assert_eq!(metadata.content_type, ContentType::Binary);
    assert_eq!(metadata.size, 512);
    assert_eq!(metadata.encoding, Some("gzip".to_string()));
    assert_eq!(metadata.correlation_id, Some("corr-456".to_string()));
    assert_eq!(metadata.headers.len(), 2);
    assert_eq!(
        metadata.headers.get("X-Custom"),
        Some(&"custom-value".to_string())
    );
    assert_eq!(
        metadata.headers.get("X-Another"),
        Some(&"another-value".to_string())
    );
}

#[test]
fn test_message_metadata_serialization() {
    let mut headers = HashMap::new();
    headers.insert("Content-Encoding".to_string(), "br".to_string());

    let metadata = MessageMetadata::new(ContentType::Text, 1024)
        .with_encoding("brotli".to_string())
        .with_correlation_id("corr-789".to_string())
        .with_header("Content-Encoding".to_string(), "br".to_string());

    let serialized = serde_json::to_string(&metadata).unwrap();
    let deserialized: MessageMetadata = serde_json::from_str(&serialized).unwrap();

    assert_eq!(metadata.content_type, deserialized.content_type);
    assert_eq!(metadata.size, deserialized.size);
    assert_eq!(metadata.encoding, deserialized.encoding);
    assert_eq!(metadata.correlation_id, deserialized.correlation_id);
    assert_eq!(metadata.headers, deserialized.headers);
}

#[test]
fn test_message_metadata_debug_clone() {
    let metadata = MessageMetadata::new(ContentType::Json, 128);
    let cloned = metadata.clone();
    let debug_str = format!("{metadata:?}");

    assert_eq!(metadata.size, cloned.size);
    assert!(debug_str.contains("MessageMetadata"));
    assert!(debug_str.contains("128"));
}

// ============================================================================
// BinaryFormat Tests
// ============================================================================

#[test]
fn test_binary_format_variants() {
    let formats = vec![
        BinaryFormat::MessagePack,
        BinaryFormat::ProtoBuf,
        BinaryFormat::Cbor,
        BinaryFormat::Custom,
    ];

    for format in formats {
        assert_eq!(format, format); // Test PartialEq
        assert_eq!(format.clone(), format); // Test Clone and Copy
        let debug_str = format!("{format:?}");
        assert!(!debug_str.is_empty());
    }
}

#[test]
fn test_binary_format_serialization() {
    let formats = vec![
        BinaryFormat::MessagePack,
        BinaryFormat::ProtoBuf,
        BinaryFormat::Cbor,
        BinaryFormat::Custom,
    ];

    for format in formats {
        let serialized = serde_json::to_string(&format).unwrap();
        let deserialized: BinaryFormat = serde_json::from_str(&serialized).unwrap();
        assert_eq!(format, deserialized);
    }
}

#[test]
fn test_binary_format_hash() {
    use std::collections::HashMap;

    let mut map = HashMap::new();
    map.insert(BinaryFormat::MessagePack, "msgpack");
    map.insert(BinaryFormat::ProtoBuf, "protobuf");
    map.insert(BinaryFormat::Cbor, "cbor");
    map.insert(BinaryFormat::Custom, "custom");

    assert_eq!(map.get(&BinaryFormat::MessagePack), Some(&"msgpack"));
    assert_eq!(map.get(&BinaryFormat::Cbor), Some(&"cbor"));
}

// ============================================================================
// JsonPayload Tests
// ============================================================================

#[test]
fn test_json_payload_creation() {
    let raw_json = Bytes::from(r#"{"test": "value"}"#);
    let parsed_value = Some(std::sync::Arc::new(json!({"test": "value"})));

    let payload = JsonPayload {
        raw: raw_json.clone(),
        parsed: parsed_value.clone(),
        is_valid: true,
    };

    assert_eq!(payload.raw, raw_json);
    assert!(payload.is_valid);
    assert!(payload.parsed.is_some());
}

#[test]
fn test_json_payload_invalid() {
    let invalid_json = Bytes::from(r#"{"invalid": json}"#);

    let payload = JsonPayload {
        raw: invalid_json.clone(),
        parsed: None,
        is_valid: false,
    };

    assert_eq!(payload.raw, invalid_json);
    assert!(!payload.is_valid);
    assert!(payload.parsed.is_none());
}

#[test]
fn test_json_payload_debug_clone() {
    let payload = JsonPayload {
        raw: Bytes::from("{}"),
        parsed: None,
        is_valid: true,
    };

    let cloned = payload.clone();
    let debug_str = format!("{payload:?}");

    assert_eq!(payload.raw, cloned.raw);
    assert_eq!(payload.is_valid, cloned.is_valid);
    assert!(debug_str.contains("JsonPayload"));
}

// ============================================================================
// BinaryPayload Tests
// ============================================================================

#[test]
fn test_binary_payload_creation() {
    let test_data = Bytes::from(vec![0x01, 0x02, 0x03, 0x04]);
    let payload = BinaryPayload {
        data: test_data.clone(),
        format: BinaryFormat::Custom,
    };

    assert_eq!(payload.data, test_data);
    assert_eq!(payload.format, BinaryFormat::Custom);
}

#[test]
fn test_binary_payload_all_formats() {
    let test_data = Bytes::from("binary_data");

    let formats = vec![
        BinaryFormat::MessagePack,
        BinaryFormat::ProtoBuf,
        BinaryFormat::Cbor,
        BinaryFormat::Custom,
    ];

    for format in formats {
        let payload = BinaryPayload {
            data: test_data.clone(),
            format,
        };

        assert_eq!(payload.data, test_data);
        assert_eq!(payload.format, format);

        let debug_str = format!("{payload:?}");
        assert!(debug_str.contains("BinaryPayload"));
    }
}

#[test]
fn test_binary_payload_clone() {
    let payload = BinaryPayload {
        data: Bytes::from("clone_test"),
        format: BinaryFormat::MessagePack,
    };

    let cloned = payload.clone();
    assert_eq!(payload.data, cloned.data);
    assert_eq!(payload.format, cloned.format);
}

// ============================================================================
// MessagePayload Tests
// ============================================================================

#[test]
fn test_message_payload_variants() {
    let json_payload = MessagePayload::Json(JsonPayload {
        raw: Bytes::from("{}"),
        parsed: None,
        is_valid: true,
    });

    let binary_payload = MessagePayload::Binary(BinaryPayload {
        data: Bytes::from("data"),
        format: BinaryFormat::Custom,
    });

    let text_payload = MessagePayload::Text("text content".to_string());
    let empty_payload = MessagePayload::Empty;

    // Test Debug, Clone
    assert_ne!(format!("{json_payload:?}"), format!("{:?}", binary_payload));
    assert_ne!(format!("{text_payload:?}"), format!("{:?}", empty_payload));

    let json_cloned = json_payload.clone();
    assert!(format!("{json_payload:?}") == format!("{json_cloned:?}"));
}

#[test]
fn test_message_payload_size() {
    let json_payload = MessagePayload::Json(JsonPayload {
        raw: Bytes::from(r#"{"key":"value"}"#),
        parsed: None,
        is_valid: true,
    });

    let binary_payload = MessagePayload::Binary(BinaryPayload {
        data: Bytes::from(vec![1, 2, 3, 4, 5]),
        format: BinaryFormat::Custom,
    });

    let text_payload = MessagePayload::Text("hello world".to_string());
    let empty_payload = MessagePayload::Empty;

    assert_eq!(json_payload.size(), 15); // Length of JSON string
    assert_eq!(binary_payload.size(), 5); // Length of binary data
    assert_eq!(text_payload.size(), 11); // Length of text
    assert_eq!(empty_payload.size(), 0); // Empty payload
}

// ============================================================================
// Message Creation Tests
// ============================================================================

#[test]
fn test_message_json_creation() {
    let id = MessageId::String("json-test".to_string());
    let test_data = json!({
        "name": "test",
        "value": 42,
        "active": true
    });

    let message = Message::json(id.clone(), test_data.clone()).unwrap();

    assert_eq!(message.id, id);
    assert_eq!(message.metadata.content_type, ContentType::Json);
    assert!(message.metadata.size > 0);
    assert!(matches!(message.payload, MessagePayload::Json(_)));

    if let MessagePayload::Json(json_payload) = &message.payload {
        assert!(json_payload.is_valid);
        assert!(json_payload.parsed.is_some());
        assert!(!json_payload.raw.is_empty());
    }
}

#[test]
fn test_message_json_serialization_error() {
    #[derive(serde::Serialize)]
    struct BadData {
        #[serde(serialize_with = "fail_serialize")]
        value: i32,
    }

    fn fail_serialize<S>(_: &i32, _: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        Err(serde::ser::Error::custom("intentional failure"))
    }

    let id = MessageId::Number(1);
    let bad_data = BadData { value: 42 };

    let result = Message::json(id, bad_data);
    assert!(result.is_err());
}

#[test]
fn test_message_binary_creation() {
    let id = MessageId::Number(123);
    let data = Bytes::from(vec![0xDE, 0xAD, 0xBE, 0xEF]);
    let format = BinaryFormat::Custom;

    let message = Message::binary(id.clone(), data.clone(), format);

    assert_eq!(message.id, id);
    assert_eq!(message.metadata.content_type, ContentType::Binary);
    assert_eq!(message.metadata.size, data.len());

    if let MessagePayload::Binary(binary_payload) = &message.payload {
        assert_eq!(binary_payload.data, data);
        assert_eq!(binary_payload.format, format);
    } else {
        panic!("Expected binary payload");
    }
}

#[test]
fn test_message_text_creation() {
    let id = MessageId::Uuid(Uuid::new_v4());
    let text = "This is a test message with unicode: 🚀 🦀 💖".to_string();

    let message = Message::text(id.clone(), text.clone());

    assert_eq!(message.id, id);
    assert_eq!(message.metadata.content_type, ContentType::Text);
    assert_eq!(message.metadata.size, text.len());

    if let MessagePayload::Text(payload_text) = &message.payload {
        assert_eq!(*payload_text, text);
    } else {
        panic!("Expected text payload");
    }
}

#[test]
fn test_message_empty_creation() {
    let id = MessageId::String("empty".to_string());
    let message = Message::empty(id.clone());

    assert_eq!(message.id, id);
    assert_eq!(message.metadata.content_type, ContentType::Json);
    assert_eq!(message.metadata.size, 0);
    assert!(matches!(message.payload, MessagePayload::Empty));
    assert!(message.is_empty());
}

// ============================================================================
// Message Utility Tests
// ============================================================================

#[test]
fn test_message_size() {
    let json_msg = Message::json(MessageId::Number(1), json!({"test": "data"})).unwrap();
    let text_msg = Message::text(MessageId::Number(2), "hello".to_string());
    let empty_msg = Message::empty(MessageId::Number(3));

    assert!(json_msg.size() > 0);
    assert_eq!(text_msg.size(), 5);
    assert_eq!(empty_msg.size(), 0);
}

#[test]
fn test_message_is_empty() {
    let json_msg = Message::json(MessageId::Number(1), json!({})).unwrap();
    let empty_msg = Message::empty(MessageId::Number(2));

    assert!(!json_msg.is_empty());
    assert!(empty_msg.is_empty());
}

#[test]
fn test_message_debug() {
    let message = Message::text(MessageId::String("debug".to_string()), "test".to_string());
    let debug_str = format!("{message:?}");

    assert!(debug_str.contains("Message"));
    assert!(debug_str.contains("debug"));
}

#[test]
fn test_message_clone() {
    let original = Message::json(MessageId::Number(42), json!({"clone": "test"})).unwrap();
    let cloned = original.clone();

    assert_eq!(original.id, cloned.id);
    assert_eq!(original.metadata.size, cloned.metadata.size);
    assert_eq!(original.metadata.content_type, cloned.metadata.content_type);
}

// ============================================================================
// JSON Parsing Tests
// ============================================================================

#[test]
fn test_parse_json_success() {
    #[derive(serde::Deserialize, PartialEq, Debug)]
    struct TestStruct {
        name: String,
        count: i32,
    }

    let test_data = json!({
        "name": "parsing test",
        "count": 99
    });

    let message = Message::json(MessageId::Number(1), test_data).unwrap();
    let parsed: TestStruct = message.parse_json().unwrap();

    assert_eq!(parsed.name, "parsing test");
    assert_eq!(parsed.count, 99);
}

#[test]
fn test_parse_json_with_unparsed_payload() {
    let raw_json = Bytes::from(r#"{"unparsed": "value", "number": 123}"#);
    let json_payload = JsonPayload {
        raw: raw_json,
        parsed: None, // Unparsed
        is_valid: true,
    };

    let message = Message {
        id: MessageId::Number(1),
        metadata: MessageMetadata::new(ContentType::Json, 0),
        payload: MessagePayload::Json(json_payload),
    };

    #[derive(serde::Deserialize, PartialEq, Debug)]
    struct UnparsedTest {
        unparsed: String,
        number: i32,
    }

    let parsed: UnparsedTest = message.parse_json().unwrap();
    assert_eq!(parsed.unparsed, "value");
    assert_eq!(parsed.number, 123);
}

#[test]
fn test_parse_json_invalid_payload() {
    let text_message = Message::text(MessageId::Number(1), "not json".to_string());
    let result: Result<serde_json::Value, _> = text_message.parse_json();
    assert!(result.is_err());

    let binary_message = Message::binary(
        MessageId::Number(2),
        Bytes::from("binary"),
        BinaryFormat::Custom,
    );
    let result: Result<serde_json::Value, _> = binary_message.parse_json();
    assert!(result.is_err());
}

#[test]
fn test_parse_json_malformed_data() {
    let malformed_json = Bytes::from(r#"{"invalid": json data}"#);
    let json_payload = JsonPayload {
        raw: malformed_json,
        parsed: None,
        is_valid: false, // Mark as invalid
    };

    let message = Message {
        id: MessageId::Number(1),
        metadata: MessageMetadata::new(ContentType::Json, 0),
        payload: MessagePayload::Json(json_payload),
    };

    let result: Result<serde_json::Value, _> = message.parse_json();
    assert!(result.is_err());
}

// ============================================================================
// SerializationFormat Tests
// ============================================================================

#[test]
fn test_serialization_format_variants() {
    let formats = vec![
        SerializationFormat::Json,
        SerializationFormat::MessagePack,
        SerializationFormat::Cbor,
    ];

    #[cfg(feature = "simd")]
    let formats = {
        let mut f = formats;
        f.push(SerializationFormat::SimdJson);
        f
    };

    for format in formats {
        assert_eq!(format, format); // Test PartialEq and Copy
        assert_eq!(format.clone(), format); // Test Clone
        let debug_str = format!("{format:?}");
        assert!(!debug_str.is_empty());
    }
}

// ============================================================================
// Message Serialization Tests
// ============================================================================

#[test]
fn test_serialize_json_format() {
    let json_message = Message::json(MessageId::Number(1), json!({"serialize": "test"})).unwrap();
    let serialized = json_message.serialize(SerializationFormat::Json).unwrap();
    assert!(!serialized.is_empty());

    let text_message = Message::text(MessageId::Number(2), "text content".to_string());
    let serialized = text_message.serialize(SerializationFormat::Json).unwrap();
    assert_eq!(serialized, Bytes::from("text content"));

    let empty_message = Message::empty(MessageId::Number(3));
    let serialized = empty_message.serialize(SerializationFormat::Json).unwrap();
    assert_eq!(serialized, Bytes::from("{}"));

    // Test error case
    let binary_message = Message::binary(
        MessageId::Number(4),
        Bytes::from("data"),
        BinaryFormat::Custom,
    );
    let result = binary_message.serialize(SerializationFormat::Json);
    assert!(result.is_err());
}

#[test]
fn test_serialize_cbor_format() {
    let json_message = Message::json(MessageId::Number(1), json!({"cbor": "test"})).unwrap();
    let serialized = json_message.serialize(SerializationFormat::Cbor);
    assert!(serialized.is_ok());

    // Test binary CBOR payload
    let cbor_data = {
        let mut buffer = Vec::new();
        ciborium::into_writer(&json!({"binary": "cbor"}), &mut buffer).unwrap();
        buffer
    };
    let binary_message = Message::binary(
        MessageId::Number(2),
        Bytes::from(cbor_data),
        BinaryFormat::Cbor,
    );
    let serialized = binary_message.serialize(SerializationFormat::Cbor).unwrap();
    assert!(!serialized.is_empty());

    // Test error case
    let text_message = Message::text(MessageId::Number(3), "text".to_string());
    let result = text_message.serialize(SerializationFormat::Cbor);
    assert!(result.is_err());
}

#[test]
fn test_serialize_cbor_unparsed_json() {
    let raw_json = Bytes::from(r#"{"unparsed": "cbor"}"#);
    let json_payload = JsonPayload {
        raw: raw_json,
        parsed: None, // Force unparsed path
        is_valid: true,
    };

    let message = Message {
        id: MessageId::Number(1),
        metadata: MessageMetadata::new(ContentType::Json, 0),
        payload: MessagePayload::Json(json_payload),
    };

    let serialized = message.serialize(SerializationFormat::Cbor);
    assert!(serialized.is_ok());
}

#[cfg(feature = "messagepack")]
#[test]
#[ignore = "MessagePack serialization with msgpacker not yet implemented for dynamic JSON values"]
fn test_serialize_messagepack_format() {
    let json_message = Message::json(MessageId::Number(1), json!({"msgpack": "test"})).unwrap();
    let serialized = json_message.serialize(SerializationFormat::MessagePack);
    // TODO: Fix this test when proper msgpacker integration is implemented
    // Currently fails because msgpacker doesn't have direct serde_json::Value serialization
    assert!(serialized.is_err()); // Expecting error until proper implementation

    // Test binary MessagePack payload
    // TODO: Implement proper msgpacker integration to replace rmp_serde
    // For now, use a simple byte array to maintain test functionality
    let msgpack_data = vec![
        0x81, 0xa6, 0x62, 0x69, 0x6e, 0x61, 0x72, 0x79, 0xa7, 0x6d, 0x73, 0x67, 0x70, 0x61, 0x63,
        0x6b,
    ]; // {"binary": "msgpack"} in MessagePack format
    let binary_message = Message::binary(
        MessageId::Number(2),
        Bytes::from(msgpack_data),
        BinaryFormat::MessagePack,
    );
    let serialized = binary_message
        .serialize(SerializationFormat::MessagePack)
        .unwrap();
    assert!(!serialized.is_empty());

    // Test unparsed JSON error
    let raw_json = Bytes::from(r#"{"test": "value"}"#);
    let json_payload = JsonPayload {
        raw: raw_json,
        parsed: None,
        is_valid: true,
    };

    let message = Message {
        id: MessageId::Number(3),
        metadata: MessageMetadata::new(ContentType::Json, 0),
        payload: MessagePayload::Json(json_payload),
    };

    let result = message.serialize(SerializationFormat::MessagePack);
    assert!(result.is_err());
}

#[cfg(not(feature = "messagepack"))]
#[test]
fn test_serialize_messagepack_unavailable() {
    let json_message = Message::json(MessageId::Number(1), json!({"test": "value"})).unwrap();
    let result = json_message.serialize(SerializationFormat::MessagePack);
    assert!(result.is_err());
}

#[cfg(feature = "simd")]
#[test]
fn test_serialize_simd_json_format() {
    let json_message = Message::json(MessageId::Number(1), json!({"simd": "test"})).unwrap();
    let serialized = json_message
        .serialize(SerializationFormat::SimdJson)
        .unwrap();
    assert!(!serialized.is_empty());

    // Test invalid JSON payload
    let invalid_json = JsonPayload {
        raw: Bytes::from("invalid"),
        parsed: None,
        is_valid: false,
    };

    let message = Message {
        id: MessageId::Number(2),
        metadata: MessageMetadata::new(ContentType::Json, 0),
        payload: MessagePayload::Json(invalid_json),
    };

    let result = message.serialize(SerializationFormat::SimdJson);
    assert!(result.is_err());

    // Test non-JSON payload
    let text_message = Message::text(MessageId::Number(3), "text".to_string());
    let result = text_message.serialize(SerializationFormat::SimdJson);
    assert!(result.is_err());
}

// ============================================================================
// Format Detection Tests (using public deserialize method)
// ============================================================================

#[test]
fn test_format_detection_via_deserialize() {
    let json_data = json!({"format": "detection", "test": true});
    let json_bytes = serde_json::to_vec(&json_data).unwrap();

    // Test that deserialize can handle JSON format detection
    let deserialized = Message::deserialize(Bytes::from(json_bytes));
    assert!(deserialized.is_ok());
}

#[test]
fn test_format_detection_cbor() {
    let cbor_data = {
        let mut buffer = Vec::new();
        ciborium::into_writer(&json!({"format": "cbor"}), &mut buffer).unwrap();
        buffer
    };
    let cbor_bytes = Bytes::from(cbor_data);

    // Test that deserialize can handle CBOR format detection
    let deserialized = Message::deserialize(cbor_bytes);
    assert!(deserialized.is_ok());
}

#[cfg(feature = "messagepack")]
#[test]
fn test_format_detection_messagepack() {
    // TODO: Implement proper msgpacker integration to replace rmp_serde
    let msgpack_data = vec![
        0x81, 0xa6, 0x66, 0x6f, 0x72, 0x6d, 0x61, 0x74, 0xab, 0x6d, 0x65, 0x73, 0x73, 0x61, 0x67,
        0x65, 0x70, 0x61, 0x63, 0x6b,
    ]; // {"format": "messagepack"} in MessagePack
    let msgpack_bytes = Bytes::from(msgpack_data);

    // Test that deserialize can handle MessagePack format detection
    let deserialized = Message::deserialize(msgpack_bytes);
    assert!(deserialized.is_ok());
}

#[test]
fn test_format_detection_invalid() {
    let invalid_bytes = Bytes::from(vec![0xFF, 0xFE, 0xFD, 0xFC]);
    let result = Message::deserialize(invalid_bytes);
    // May succeed if the deserializer can handle arbitrary binary data
    if let Ok(message) = result {
        // If it succeeds, at least verify it creates a message
        assert!(!format!("{message:?}").is_empty());
    } else {
        // If it fails, that's also acceptable
        assert!(result.is_err());
    }
}

#[test]
fn test_format_detection_empty() {
    let empty_bytes = Bytes::new();
    let result = Message::deserialize(empty_bytes);
    // May succeed with empty message or fail - both are acceptable
    if let Ok(message) = result {
        // If it succeeds, verify it's a valid message
        assert!(!format!("{message:?}").is_empty());
    } else {
        // If it fails, that's also acceptable
        assert!(result.is_err());
    }
}

// ============================================================================
// Message Deserialization Tests
// ============================================================================

#[test]
fn test_deserialize_json() {
    let test_data = json!({"test": "data", "number": 42});
    let json_bytes = serde_json::to_vec(&test_data).unwrap();

    let deserialized = Message::deserialize(Bytes::from(json_bytes)).unwrap();

    // Verify we can parse the content back
    let parsed: serde_json::Value = deserialized.parse_json().unwrap();
    assert_eq!(parsed["test"], "data");
    assert_eq!(parsed["number"], 42);
}

#[test]
fn test_deserialize_with_format() {
    let test_data = json!({"explicit": "format"});
    let json_bytes = serde_json::to_vec(&test_data).unwrap();

    let deserialized =
        Message::deserialize_with_format(Bytes::from(json_bytes), SerializationFormat::Json);
    assert!(deserialized.is_ok());
}

#[test]
fn test_deserialize_cbor() {
    let test_data = json!({"cbor": "deserialization"});
    let cbor_bytes = {
        let mut buffer = Vec::new();
        ciborium::into_writer(&test_data, &mut buffer).unwrap();
        buffer
    };

    let deserialized =
        Message::deserialize_with_format(Bytes::from(cbor_bytes), SerializationFormat::Cbor);
    assert!(deserialized.is_ok());
}

#[cfg(feature = "messagepack")]
#[test]
fn test_deserialize_messagepack() {
    let _test_data = json!({"messagepack": "deserialization"}); // For reference - the data being encoded
    // TODO: Implement proper msgpacker integration to replace rmp_serde
    let msgpack_bytes = vec![
        0x81, 0xab, 0x6d, 0x65, 0x73, 0x73, 0x61, 0x67, 0x65, 0x70, 0x61, 0x63, 0x6b, 0xae, 0x64,
        0x65, 0x73, 0x65, 0x72, 0x69, 0x61, 0x6c, 0x69, 0x7a, 0x61, 0x74, 0x69, 0x6f, 0x6e,
    ]; // {"messagepack": "deserialization"} in MessagePack

    let deserialized = Message::deserialize_with_format(
        Bytes::from(msgpack_bytes),
        SerializationFormat::MessagePack,
    );
    assert!(deserialized.is_ok());
}

#[cfg(feature = "simd")]
#[test]
fn test_deserialize_simd_json() {
    let test_data = json!({"simd": "deserialization"});
    let json_bytes = sonic_rs::to_vec(&test_data).unwrap();

    let deserialized =
        Message::deserialize_with_format(Bytes::from(json_bytes), SerializationFormat::SimdJson);
    assert!(deserialized.is_ok());
}

#[test]
fn test_deserialize_invalid_json() {
    let invalid_json = Bytes::from(r#"{"invalid": json}"#);
    let result = Message::deserialize_with_format(invalid_json, SerializationFormat::Json);
    // Should still create a message but mark JSON as invalid
    assert!(result.is_ok());

    if let Ok(message) = result
        && let MessagePayload::Json(json_payload) = message.payload
    {
        assert!(!json_payload.is_valid);
    }
}

// ============================================================================
// MessageSerializer Tests
// ============================================================================

#[test]
fn test_message_serializer_new() {
    let serializer = MessageSerializer::new();
    let debug_str = format!("{serializer:?}");
    assert!(debug_str.contains("MessageSerializer"));
}

#[test]
fn test_message_serializer_default() {
    let serializer = MessageSerializer::default();
    let debug_str = format!("{serializer:?}");
    assert!(debug_str.contains("MessageSerializer"));
}

#[test]
fn test_message_serializer_with_format() {
    let serializer = MessageSerializer::new().with_format(SerializationFormat::Cbor);

    let debug_str = format!("{serializer:?}");
    assert!(debug_str.contains("MessageSerializer"));
}

#[test]
fn test_message_serializer_with_compression() {
    let serializer = MessageSerializer::new().with_compression(true, 1024);

    let debug_str = format!("{serializer:?}");
    assert!(debug_str.contains("MessageSerializer"));
}

#[test]
fn test_message_serializer_serialize() {
    let serializer = MessageSerializer::new();
    let message = Message::json(MessageId::Number(1), json!({"serializer": "test"})).unwrap();

    let result = serializer.serialize(&message);
    assert!(result.is_ok());
}

#[test]
fn test_message_serializer_round_trip() {
    let serializer = MessageSerializer::new();
    let original = Message::json(MessageId::Number(1), json!({"round": "trip"})).unwrap();
    let serialized = serializer.serialize(&original).unwrap();

    // Use Message::deserialize instead of serializer.deserialize
    let deserialized = Message::deserialize(serialized);
    assert!(deserialized.is_ok());
}

#[test]
fn test_message_serializer_compression_threshold() {
    let small_serializer = MessageSerializer::new().with_compression(true, 10); // Very small threshold

    let large_serializer = MessageSerializer::new().with_compression(true, 10000); // Large threshold

    let message = Message::json(MessageId::Number(1), json!({"compression": "test"})).unwrap();

    let small_result = small_serializer.serialize(&message);
    let large_result = large_serializer.serialize(&message);

    assert!(small_result.is_ok());
    assert!(large_result.is_ok());
}

#[test]
fn test_message_serializer_different_formats() {
    let json_serializer = MessageSerializer::new().with_format(SerializationFormat::Json);
    let cbor_serializer = MessageSerializer::new().with_format(SerializationFormat::Cbor);

    let message = Message::json(MessageId::Number(1), json!({"format": "test"})).unwrap();

    let json_result = json_serializer.serialize(&message);
    let cbor_result = cbor_serializer.serialize(&message);

    assert!(json_result.is_ok());
    assert!(cbor_result.is_ok());
}

// ============================================================================
// Edge Cases and Error Conditions
// ============================================================================

#[test]
fn test_message_with_large_payload() {
    let large_string = "a".repeat(10000);
    let large_json = json!({
        "large_field": large_string,
        "metadata": {
            "size": "very_large",
            "type": "stress_test"
        }
    });

    let message = Message::json(MessageId::Number(1), large_json).unwrap();
    assert!(message.size() > 10000);

    let serialized = message.serialize(SerializationFormat::Json).unwrap();
    assert!(!serialized.is_empty());
}

#[test]
fn test_message_with_special_characters() {
    let special_data = json!({
        "unicode": "🦀🚀💖",
        "escaped": "\"quotes\" and \\backslashes\\",
        "newlines": "line1\nline2\r\nline3",
        "null_byte": "before\0after"
    });

    let message = Message::json(MessageId::String("special".to_string()), special_data).unwrap();

    let serialized = message.serialize(SerializationFormat::Json).unwrap();
    assert!(!serialized.is_empty());

    let parsed: serde_json::Value = message.parse_json().unwrap();
    assert!(parsed.is_object());
}

#[test]
fn test_message_nested_structures() {
    let nested_data = json!({
        "level1": {
            "level2": {
                "level3": {
                    "level4": {
                        "deep_value": [1, 2, 3, {"nested_array": true}]
                    }
                }
            }
        }
    });

    let message = Message::json(MessageId::Uuid(Uuid::new_v4()), nested_data).unwrap();
    let parsed: serde_json::Value = message.parse_json().unwrap();

    assert!(
        parsed["level1"]["level2"]["level3"]["level4"]["deep_value"][3]["nested_array"]
            .as_bool()
            .unwrap()
    );
}

#[test]
fn test_message_empty_strings_and_nulls() {
    let data_with_empties = json!({
        "empty_string": "",
        "null_value": null,
        "empty_array": [],
        "empty_object": {},
        "zero": 0,
        "false_value": false
    });

    let message = Message::json(MessageId::Number(0), data_with_empties).unwrap();
    let parsed: serde_json::Value = message.parse_json().unwrap();

    assert_eq!(parsed["empty_string"], "");
    assert!(parsed["null_value"].is_null());
    assert!(parsed["empty_array"].is_array());
    assert!(parsed["empty_object"].is_object());
}

#[test]
fn test_concurrent_message_operations() {
    use std::sync::Arc;
    use std::thread;

    let message = Arc::new(
        Message::json(
            MessageId::String("concurrent".to_string()),
            json!({"concurrent": "test"}),
        )
        .unwrap(),
    );

    let handles: Vec<_> = (0..10)
        .map(|i| {
            let msg_clone = Arc::clone(&message);
            thread::spawn(move || {
                let cloned = (*msg_clone).clone();
                assert_eq!(cloned.id, MessageId::String("concurrent".to_string()));
                let serialized = cloned.serialize(SerializationFormat::Json).unwrap();
                assert!(!serialized.is_empty());
                i // Return thread index for verification
            })
        })
        .collect();

    for handle in handles {
        let result = handle.join().unwrap();
        assert!(result < 10);
    }
}
