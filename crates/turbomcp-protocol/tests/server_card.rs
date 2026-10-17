//! Server Cards against the extension's own schema and examples
//! (`tests/fixtures/server-card`, vendored from the snapshot SEP-2127 cites).

use std::path::PathBuf;

use serde_json::Value;
use turbomcp_protocol::server_card::{Icon, Input, KeyValueInput, Remote, Repository, ServerCard};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/server-card")
}

fn validator() -> jsonschema::Validator {
    let schema: Value =
        serde_json::from_str(&std::fs::read_to_string(fixtures().join("schema.json")).unwrap())
            .unwrap();
    let mut root = schema.clone();
    root["$ref"] = Value::String("#/$defs/ServerCard".into());
    jsonschema::validator_for(&root).unwrap()
}

fn examples(kind: &str) -> Vec<(String, Value)> {
    let mut out: Vec<_> = std::fs::read_dir(fixtures().join(kind))
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            let doc = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                doc,
            )
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[test]
fn every_valid_example_decodes_validates_and_round_trips() {
    let schema = validator();
    for (name, doc) in examples("valid") {
        assert!(
            schema.is_valid(&doc),
            "{name}: the vendored example is invalid"
        );
        let card: ServerCard =
            serde_json::from_value(doc.clone()).unwrap_or_else(|e| panic!("{name}: {e}"));
        card.validate().unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            serde_json::to_value(&card).unwrap(),
            doc,
            "{name} changed across a round trip"
        );
    }
}

#[test]
fn every_invalid_example_is_refused() {
    let schema = validator();
    for (name, doc) in examples("invalid") {
        assert!(
            !schema.is_valid(&doc),
            "{name}: the vendored example is valid"
        );
        let refused = serde_json::from_value::<ServerCard>(doc)
            .map_err(|e| e.to_string())
            .and_then(|card| card.validate().map_err(|e| e.to_string()));
        assert!(refused.is_err(), "{name} was accepted");
    }
}

#[test]
fn a_built_card_conforms_to_the_schema() {
    let card = ServerCard::new("com.example/weather", "Forecasts for any city.")
        .with_version("2.0.1")
        .with_title("Weather")
        .with_website_url("https://example.com/weather")
        .with_repository(Repository::new(
            "https://github.com/example/weather",
            "github",
        ))
        .with_icon(Icon::new("https://example.com/icon.png"))
        .with_remote(
            Remote::streamable_http("https://{region}.example.com/mcp")
                .with_variable("region", Input::new("Region").required().with_default("us"))
                .with_header(KeyValueInput::new(
                    "Authorization",
                    Input::new("Bearer token").required().secret(),
                ))
                .with_supported_protocol_versions(["2025-11-25", "2026-07-28"]),
        )
        .with_meta_entry("com.example/tier", serde_json::json!("free"));
    card.validate().unwrap();
    let doc = serde_json::to_value(&card).unwrap();
    let errors: Vec<String> = validator()
        .iter_errors(&doc)
        .map(|e| e.to_string())
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
}
