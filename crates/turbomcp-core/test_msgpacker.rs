#[cfg(feature = "messagepack")]
fn explore_msgpacker_api() {
    use serde_json::{json, Value};
    use msgpacker::{Message, Packable};

    // Let's explore the API by trying different approaches
    let test_value = json!({"test": "value", "number": 42});

    // Try to see what Message methods are available
    let message = match test_value {
        Value::String(s) => Message::string(s),
        _ => todo!("implement other types"),
    };

    println!("Created message: {:?}", message);
}

#[cfg(feature = "messagepack")]
#[test]
fn test_msgpacker_api_exploration() {
    explore_msgpacker_api();
}