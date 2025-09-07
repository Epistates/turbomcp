//! Comprehensive validation of all new ergonomic builder constructors

use turbomcp::elicitation_api::{checkbox, choices, integer_field, number_field, options, text};
use turbomcp_protocol::elicitation::{ElicitationSchema, PrimitiveSchemaDefinition};

#[test]
fn test_text_constructor() {
    let schema = text("Full Name").min_length(2).max_length(50);
    let def: PrimitiveSchemaDefinition = schema.into();

    if let PrimitiveSchemaDefinition::String(s) = def {
        assert_eq!(s.title, Some("Full Name".to_string()));
        assert_eq!(s.min_length, Some(2));
        assert_eq!(s.max_length, Some(50));
    } else {
        panic!("Expected String schema");
    }
}

#[test]
fn test_text_with_options() {
    let schema = text("Theme").options(&["light", "dark", "auto"]);
    let def: PrimitiveSchemaDefinition = schema.into();

    if let PrimitiveSchemaDefinition::Enum(e) = def {
        assert_eq!(e.title, Some("Theme".to_string()));
        assert_eq!(e.enum_values, vec!["light", "dark", "auto"]);
    } else {
        panic!("Expected Enum schema");
    }
}

#[test]
fn test_integer_field_constructor() {
    let schema = integer_field("Age").range(0.0, 120.0);
    let def: PrimitiveSchemaDefinition = schema.into();

    if let PrimitiveSchemaDefinition::Number(n) = def {
        assert_eq!(n.title, Some("Age".to_string()));
        assert_eq!(n.schema_type, "integer");
        assert_eq!(n.minimum, Some(0.0));
        assert_eq!(n.maximum, Some(120.0));
    } else {
        panic!("Expected Number schema");
    }
}

#[test]
fn test_number_field_constructor() {
    let schema = number_field("Temperature").range(-273.15, 1000.0);
    let def: PrimitiveSchemaDefinition = schema.into();

    if let PrimitiveSchemaDefinition::Number(n) = def {
        assert_eq!(n.title, Some("Temperature".to_string()));
        assert_eq!(n.schema_type, "number");
        assert_eq!(n.minimum, Some(-273.15));
        assert_eq!(n.maximum, Some(1000.0));
    } else {
        panic!("Expected Number schema");
    }
}

#[test]
fn test_checkbox_constructor() {
    let schema = checkbox("Enable Notifications")
        .default(true)
        .description("Get alerts");
    let def: PrimitiveSchemaDefinition = schema.into();

    if let PrimitiveSchemaDefinition::Boolean(b) = def {
        assert_eq!(b.title, Some("Enable Notifications".to_string()));
        assert_eq!(b.description, Some("Get alerts".to_string()));
        assert_eq!(b.default, Some(true));
    } else {
        panic!("Expected Boolean schema");
    }
}

#[test]
fn test_options_constructor() {
    let schema = options(&["small", "medium", "large"]).title("Size");
    let def: PrimitiveSchemaDefinition = schema.into();

    if let PrimitiveSchemaDefinition::Enum(e) = def {
        assert_eq!(e.title, Some("Size".to_string()));
        assert_eq!(e.enum_values, vec!["small", "medium", "large"]);
    } else {
        panic!("Expected Enum schema");
    }
}

#[test]
fn test_choices_constructor() {
    let schema = choices(&["yes", "no", "maybe"]).description("Your choice");
    let def: PrimitiveSchemaDefinition = schema.into();

    if let PrimitiveSchemaDefinition::Enum(e) = def {
        assert_eq!(e.description, Some("Your choice".to_string()));
        assert_eq!(e.enum_values, vec!["yes", "no", "maybe"]);
    } else {
        panic!("Expected Enum schema");
    }
}

#[test]
fn test_backward_compatibility() {
    use turbomcp::elicitation_api::{
        boolean_builder, enum_of, integer_builder, number_builder, string_builder,
    };

    // Builder patterns should still work - test actual functionality
    let old_string = string_builder().title("Name").build();
    let _old_integer = integer_builder().title("Count").build();
    let _old_number = number_builder().title("Value").build();
    let _old_boolean = boolean_builder().title("Flag").build();
    let _old_enum = enum_of(vec!["a".to_string(), "b".to_string()]).build();

    // Validate they actually work as expected
    if let PrimitiveSchemaDefinition::String(s) = old_string {
        assert_eq!(s.title, Some("Name".to_string()));
    } else {
        panic!("Expected String schema");
    }
}

#[test]
fn test_complex_realistic_form() {
    // Create a realistic form with all field types using new ergonomic constructors
    let mut schema = ElicitationSchema::new();

    schema
        .properties
        .insert("name".to_string(), text("Full Name").min_length(2).into());
    schema.properties.insert(
        "age".to_string(),
        integer_field("Age").range(18.0, 100.0).into(),
    );
    schema.properties.insert(
        "salary".to_string(),
        number_field("Annual Salary").min(0.0).into(),
    );
    schema.properties.insert(
        "theme".to_string(),
        text("UI Theme").options(&["light", "dark"]).into(),
    );
    schema.properties.insert(
        "notifications".to_string(),
        checkbox("Email Notifications").default(true).into(),
    );
    schema.properties.insert(
        "priority".to_string(),
        options(&["low", "medium", "high"]).title("Priority").into(),
    );

    // Should have 6 properties
    assert_eq!(schema.properties.len(), 6);

    // Verify each field type
    assert!(matches!(
        schema.properties.get("name"),
        Some(PrimitiveSchemaDefinition::String(_))
    ));
    assert!(matches!(
        schema.properties.get("age"),
        Some(PrimitiveSchemaDefinition::Number(_))
    ));
    assert!(matches!(
        schema.properties.get("salary"),
        Some(PrimitiveSchemaDefinition::Number(_))
    ));
    assert!(matches!(
        schema.properties.get("theme"),
        Some(PrimitiveSchemaDefinition::Enum(_))
    ));
    assert!(matches!(
        schema.properties.get("notifications"),
        Some(PrimitiveSchemaDefinition::Boolean(_))
    ));
    assert!(matches!(
        schema.properties.get("priority"),
        Some(PrimitiveSchemaDefinition::Enum(_))
    ));
}

#[test]
fn test_ergonomic_vs_traditional_comparison() {
    use turbomcp::elicitation_api::string_builder;

    // Traditional approach (still supported)
    let traditional = string_builder()
        .title("Email")
        .format(turbomcp_protocol::elicitation::StringFormat::Email)
        .build();

    // New ergonomic approach
    let ergonomic = text("Email").email();

    // Both should produce equivalent schemas
    let trad_def: PrimitiveSchemaDefinition = traditional;
    let ergo_def: PrimitiveSchemaDefinition = ergonomic.into();

    if let (PrimitiveSchemaDefinition::String(t), PrimitiveSchemaDefinition::String(e)) =
        (trad_def, ergo_def)
    {
        assert_eq!(t.title, e.title);
        assert_eq!(t.format, e.format);
    } else {
        panic!("Expected String schemas");
    }
}
