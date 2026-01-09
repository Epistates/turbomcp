//! Comprehensive tests

use super::*;

use serde_json::json;
use crate::jsonrpc::*;
use crate::types::RequestId;

#[test]
fn test_jsonrpc_version_serialization() {
    let version = JsonRpcVersion;
    let json = serde_json::to_string(&version).unwrap();
    assert_eq!(json, "\"2.0\"");
}

#[test]
fn test_jsonrpc_version_deserialization() {
    let version: JsonRpcVersion = serde_json::from_str("\"2.0\"").unwrap();
    assert_eq!(version, JsonRpcVersion);
}

#[test]
fn test_jsonrpc_version_invalid_deserialization() {
    let result = serde_json::from_str::<JsonRpcVersion>("\"1.0\"");
    assert!(result.is_err());

    let result = serde_json::from_str::<JsonRpcVersion>("\"3.0\"");
    assert!(result.is_err());
}

#[test]
fn test_jsonrpc_request_new() {
    let request = JsonRpcRequest::new(
        "test_method".to_string(),
        Some(json!({"key": "value"})),
        RequestId::String("test-id".to_string()),
    );

    assert_eq!(request.method, "test_method");
    assert!(request.params.is_some());
    assert_eq!(request.id, RequestId::String("test-id".to_string()));
}

#[test]
fn test_jsonrpc_request_without_params() {
    let request = JsonRpcRequest::without_params("test_method".to_string(), RequestId::Number(42));

    assert_eq!(request.method, "test_method");
    assert!(request.params.is_none());
    assert_eq!(request.id, RequestId::Number(42));
}

#[test]
fn test_jsonrpc_request_with_params() {
    let params = json!({"param1": "value1", "param2": 42});
    let request = JsonRpcRequest::with_params(
        "test_method".to_string(),
        params.clone(),
        RequestId::String("test-id".to_string()),
    )
    .unwrap();

    assert_eq!(request.method, "test_method");
    assert_eq!(request.params, Some(params));
}

#[test]
fn test_jsonrpc_request_with_params_valid_serialization() {
    use std::collections::HashMap;

    let mut params = HashMap::new();
    params.insert("key", "value");

    let result = JsonRpcRequest::with_params(
        "test_method".to_string(),
        params,
        RequestId::String("test-id".to_string()),
    );
    assert!(result.is_ok());
    let request = result.unwrap();
    assert_eq!(request.method, "test_method");
}

#[test]
fn test_jsonrpc_response_success() {
    let response = JsonRpcResponse::success(
        json!({"result": "success"}),
        RequestId::String("test-id".to_string()),
    );

    assert!(response.is_success());
    assert!(!response.is_error());
    assert!(response.result().is_some());
    assert!(response.error().is_none());
    assert_eq!(
        response.id,
        ResponseId::from_request(RequestId::String("test-id".to_string()))
    );
}

#[test]
fn test_jsonrpc_response_error() {
    let error = JsonRpcError::from(JsonRpcErrorCode::MethodNotFound);
    let response = JsonRpcResponse::error_response(error, RequestId::String("test-id".to_string()));

    assert!(!response.is_success());
    assert!(response.is_error());
    assert!(response.result().is_none());
    assert!(response.error().is_some());
    assert_eq!(
        response.id,
        ResponseId::from_request(RequestId::String("test-id".to_string()))
    );
}

#[test]
fn test_jsonrpc_response_parse_error() {
    let response = JsonRpcResponse::parse_error(Some("Custom parse error".to_string()));

    assert!(!response.is_success());
    assert!(response.is_error());
    assert!(response.result().is_none());
    assert!(response.error().is_some());
    assert_eq!(response.id, ResponseId::null());

    let error = response.error().unwrap();
    assert_eq!(error.code, -32700);
    assert_eq!(error.message, "Custom parse error");
}

#[test]
fn test_jsonrpc_response_parse_error_default() {
    let response = JsonRpcResponse::parse_error(None);

    let error = response.error().unwrap();
    assert_eq!(error.code, -32700);
    assert_eq!(error.message, "Parse error");
}

#[test]
fn test_jsonrpc_notification_new() {
    let notification = JsonRpcNotification::new(
        "test_notification".to_string(),
        Some(json!({"data": "value"})),
    );

    assert_eq!(notification.method, "test_notification");
    assert!(notification.params.is_some());
}

#[test]
fn test_jsonrpc_notification_without_params() {
    let notification = JsonRpcNotification::without_params("test_notification".to_string());

    assert_eq!(notification.method, "test_notification");
    assert!(notification.params.is_none());
}

#[test]
fn test_jsonrpc_notification_with_params() {
    let params = json!({"key": "value"});
    let notification =
        JsonRpcNotification::with_params("test_notification".to_string(), params.clone()).unwrap();

    assert_eq!(notification.method, "test_notification");
    assert_eq!(notification.params, Some(params));
}

#[test]
fn test_jsonrpc_notification_with_params_complex() {
    let complex_data = json!({
        "numbers": [1, 2, 3],
        "nested": {
            "key": "value",
            "boolean": true
        }
    });

    let result =
        JsonRpcNotification::with_params("test_notification".to_string(), complex_data.clone());
    assert!(result.is_ok());
    let notification = result.unwrap();
    assert_eq!(notification.method, "test_notification");
    assert_eq!(notification.params, Some(complex_data));
}

#[test]
fn test_jsonrpc_error_codes() {
    assert_eq!(JsonRpcErrorCode::ParseError.code(), -32700);
    assert_eq!(JsonRpcErrorCode::InvalidRequest.code(), -32600);
    assert_eq!(JsonRpcErrorCode::MethodNotFound.code(), -32601);
    assert_eq!(JsonRpcErrorCode::InvalidParams.code(), -32602);
    assert_eq!(JsonRpcErrorCode::InternalError.code(), -32603);

    let app_error = JsonRpcErrorCode::ApplicationError(-32001);
    assert_eq!(app_error.code(), -32001);
}

#[test]
fn test_jsonrpc_error_messages() {
    assert_eq!(JsonRpcErrorCode::ParseError.message(), "Parse error");
    assert_eq!(
        JsonRpcErrorCode::InvalidRequest.message(),
        "Invalid Request"
    );
    assert_eq!(
        JsonRpcErrorCode::MethodNotFound.message(),
        "Method not found"
    );
    assert_eq!(JsonRpcErrorCode::InvalidParams.message(), "Invalid params");
    assert_eq!(JsonRpcErrorCode::InternalError.message(), "Internal error");
    assert_eq!(
        JsonRpcErrorCode::ApplicationError(-32001).message(),
        "Application error"
    );
}

#[test]
fn test_jsonrpc_error_display() {
    let parse_error = JsonRpcErrorCode::ParseError;
    assert_eq!(format!("{parse_error}"), "Parse error (-32700)");

    let app_error = JsonRpcErrorCode::ApplicationError(-32001);
    assert_eq!(format!("{app_error}"), "Application error (-32001)");
}

#[test]
fn test_jsonrpc_error_from_code() {
    let error = JsonRpcError::from(JsonRpcErrorCode::MethodNotFound);
    assert_eq!(error.code, -32601);
    assert_eq!(error.message, "Method not found");
    assert!(error.data.is_none());
}

#[test]
fn test_jsonrpc_error_from_i32() {
    let parse_error: JsonRpcErrorCode = (-32700).into();
    assert_eq!(parse_error, JsonRpcErrorCode::ParseError);

    let invalid_request: JsonRpcErrorCode = (-32600).into();
    assert_eq!(invalid_request, JsonRpcErrorCode::InvalidRequest);

    let method_not_found: JsonRpcErrorCode = (-32601).into();
    assert_eq!(method_not_found, JsonRpcErrorCode::MethodNotFound);

    let invalid_params: JsonRpcErrorCode = (-32602).into();
    assert_eq!(invalid_params, JsonRpcErrorCode::InvalidParams);

    let internal_error: JsonRpcErrorCode = (-32603).into();
    assert_eq!(internal_error, JsonRpcErrorCode::InternalError);

    let app_error: JsonRpcErrorCode = (-32001).into();
    assert_eq!(app_error, JsonRpcErrorCode::ApplicationError(-32001));
}

#[test]
fn test_jsonrpc_message_request() {
    let request = JsonRpcRequest::without_params("method1".to_string(), RequestId::Number(1));
    let message = JsonRpcMessage::Request(request.clone());

    match message {
        JsonRpcMessage::Request(r) => assert_eq!(r.method, request.method),
        _ => panic!("Expected Request variant"),
    }
}

#[test]
fn test_jsonrpc_message_response() {
    let response = JsonRpcResponse::success(json!({"ok": true}), RequestId::Number(1));
    let message = JsonRpcMessage::Response(response.clone());

    match message {
        JsonRpcMessage::Response(r) => assert_eq!(r.id, response.id),
        _ => panic!("Expected Response variant"),
    }
}

#[test]
fn test_jsonrpc_message_notification() {
    let notification = JsonRpcNotification::without_params("test_notification".to_string());
    let message = JsonRpcMessage::Notification(notification.clone());

    match message {
        JsonRpcMessage::Notification(n) => assert_eq!(n.method, notification.method),
        _ => panic!("Expected Notification variant"),
    }
}

#[test]
fn test_utils_parse_message() {
    let json = r#"{"jsonrpc":"2.0","method":"test","id":"123"}"#;
    let message = utils::parse_message(json).unwrap();

    match message {
        JsonRpcMessage::Request(request) => {
            assert_eq!(request.method, "test");
            assert_eq!(request.id, RequestId::String("123".to_string()));
        }
        _ => panic!("Expected Request message"),
    }
}

#[test]
fn test_utils_parse_message_invalid() {
    let json = r#"{"invalid": "json"}"#;
    let result = utils::parse_message(json);
    assert!(result.is_err());
}

#[test]
fn test_utils_serialize_message() {
    let request = JsonRpcRequest::without_params("test".to_string(), RequestId::Number(1));
    let message = JsonRpcMessage::Request(request);

    let json = utils::serialize_message(&message).unwrap();
    assert!(json.contains("\"jsonrpc\":\"2.0\""));
    assert!(json.contains("\"method\":\"test\""));
    assert!(json.contains("\"id\":1"));
}

#[test]
fn test_utils_extract_method() {
    let json = r#"{"jsonrpc":"2.0","method":"test_method","id":"123"}"#;
    let method = utils::extract_method(json);
    assert_eq!(method, Some("test_method".to_string()));

    let json_without_method = r#"{"jsonrpc":"2.0","id":"123"}"#;
    let method = utils::extract_method(json_without_method);
    assert_eq!(method, None);

    let invalid_json = r#"{"invalid": json}"#;
    let method = utils::extract_method(invalid_json);
    assert_eq!(method, None);

    let json_with_non_string_method = r#"{"jsonrpc":"2.0","method":123,"id":"123"}"#;
    let method = utils::extract_method(json_with_non_string_method);
    assert_eq!(method, None);
}

#[test]
fn test_serialization_deserialization_roundtrip() {
    let request = JsonRpcRequest::new(
        "test_method".to_string(),
        Some(json!({"param1": "value1", "param2": 42})),
        RequestId::String("test-id".to_string()),
    );

    let json = serde_json::to_string(&request).unwrap();
    let parsed: JsonRpcRequest = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed.method, request.method);
    assert_eq!(parsed.params, request.params);
    assert_eq!(parsed.id, request.id);
}

#[test]
fn test_response_serialization_deserialization_roundtrip() {
    let response = JsonRpcResponse::success(
        json!({"result": "success", "data": [1, 2, 3]}),
        RequestId::Number(42),
    );

    let json = serde_json::to_string(&response).unwrap();
    let parsed: JsonRpcResponse = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed.result(), response.result());
    // Compare error presence instead of equality
    assert_eq!(parsed.error().is_some(), response.error().is_some());
    assert_eq!(parsed.id, response.id);
    assert!(parsed.is_success());
}

#[test]
fn test_notification_serialization_deserialization_roundtrip() {
    let notification = JsonRpcNotification::new(
        "test_notification".to_string(),
        Some(json!({"event": "user_action", "timestamp": 1234567890})),
    );

    let json = serde_json::to_string(&notification).unwrap();
    let parsed: JsonRpcNotification = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed.method, notification.method);
    assert_eq!(parsed.params, notification.params);
}

#[test]
fn test_error_response_with_data() {
    let error = JsonRpcError {
        code: -32001,
        message: "Application error".to_string(),
        data: Some(json!({"details": "Additional error information"})),
    };

    let response = JsonRpcResponse::error_response(error.clone(), RequestId::Number(123));

    assert!(response.is_error());
    assert_eq!(response.error().unwrap().code, error.code);
    assert_eq!(response.error().unwrap().message, error.message);
    assert_eq!(response.error().unwrap().data, error.data);
}

#[test]
fn test_request_with_numeric_id() {
    let request = JsonRpcRequest::new("test_method".to_string(), None, RequestId::Number(0));

    let json = serde_json::to_string(&request).unwrap();
    let parsed: JsonRpcRequest = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed.id, RequestId::Number(0));
}

#[test]
fn test_complex_params_serialization() {
    let complex_params = json!({
        "nested": {
            "array": [1, 2, 3, {"key": "value"}],
            "boolean": true,
            "null_value": null,
            "string": "test string with unicode: 测试"
        },
        "numbers": {
            "integer": 42,
            "float": std::f64::consts::PI,
            "negative": -123
        }
    });

    let request = JsonRpcRequest::new(
        "complex_method".to_string(),
        Some(complex_params.clone()),
        RequestId::String("complex-test".to_string()),
    );

    let json = serde_json::to_string(&request).unwrap();
    let parsed: JsonRpcRequest = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed.params, Some(complex_params));
}

#[test]
fn test_empty_method_name() {
    let request = JsonRpcRequest::without_params(String::new(), RequestId::Number(1));
    assert_eq!(request.method, "");

    let json = serde_json::to_string(&request).unwrap();
    let parsed: JsonRpcRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.method, "");
}

