//! Fuzz target for capability negotiation parsing
//!
//! This fuzzer tests the robustness of client/server capability
//! parsing and negotiation logic.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use turbomcp_protocol::types::*;

/// Structured input for capability fuzzing
#[derive(Debug, Arbitrary)]
struct CapabilityFuzzInput {
    // Server capabilities
    has_tools: bool,
    tools_list_changed: bool,
    has_prompts: bool,
    prompts_list_changed: bool,
    has_resources: bool,
    resources_list_changed: bool,
    resources_subscribe: bool,
    has_logging: bool,
    has_completions: bool,

    // Client capabilities
    has_roots: bool,
    roots_list_changed: bool,
    has_sampling: bool,
    has_elicitation: bool,
    elicitation_schema_validation: bool,

    // Experimental
    has_experimental: bool,
    experimental_keys: Vec<String>,
}

fuzz_target!(|data: &[u8]| {
    // Strategy 1: Raw JSON parsing
    if let Ok(s) = std::str::from_utf8(data) {
        // Server capabilities
        let _ = serde_json::from_str::<ServerCapabilities>(s);
        let _ = serde_json::from_str::<ToolsCapabilities>(s);
        let _ = serde_json::from_str::<PromptsCapabilities>(s);
        let _ = serde_json::from_str::<ResourcesCapabilities>(s);
        let _ = serde_json::from_str::<LoggingCapabilities>(s);

        // Client capabilities
        let _ = serde_json::from_str::<ClientCapabilities>(s);
        let _ = serde_json::from_str::<RootsCapabilities>(s);
        let _ = serde_json::from_str::<SamplingCapabilities>(s);
        let _ = serde_json::from_str::<ElicitationCapabilities>(s);

        // Initialize request/response (includes capabilities)
        let _ = serde_json::from_str::<InitializeRequest>(s);
        let _ = serde_json::from_str::<InitializeResult>(s);

        // Implementation info
        let _ = serde_json::from_str::<Implementation>(s);
    }

    // Strategy 2: Structured capability generation
    if let Ok(input) = CapabilityFuzzInput::arbitrary(&mut arbitrary::Unstructured::new(data)) {
        // Build server capabilities
        let server_caps = build_server_capabilities(&input);

        // Test serialization roundtrip
        if let Ok(json) = serde_json::to_string(&server_caps) {
            let _ = serde_json::from_str::<ServerCapabilities>(&json);
        }

        // Build client capabilities
        let client_caps = build_client_capabilities(&input);

        // Test serialization roundtrip
        if let Ok(json) = serde_json::to_string(&client_caps) {
            let _ = serde_json::from_str::<ClientCapabilities>(&json);
        }

        // Test full initialize flow
        let init_request = InitializeRequest {
            protocol_version: "2025-11-25".to_string(),
            capabilities: client_caps.clone(),
            client_info: Implementation {
                name: "fuzz-client".to_string(),
                title: None,
                version: "1.0.0".to_string(),
            },
            _meta: None,
        };

        if let Ok(json) = serde_json::to_string(&init_request) {
            let _ = serde_json::from_str::<InitializeRequest>(&json);
        }

        let init_result = InitializeResult {
            protocol_version: "2025-11-25".to_string(),
            capabilities: server_caps.clone(),
            server_info: Implementation {
                name: "fuzz-server".to_string(),
                title: None,
                version: "1.0.0".to_string(),
            },
            instructions: Some("Fuzz test instructions".to_string()),
            _meta: None,
        };

        if let Ok(json) = serde_json::to_string(&init_result) {
            let _ = serde_json::from_str::<InitializeResult>(&json);
        }
    }
});

fn build_server_capabilities(input: &CapabilityFuzzInput) -> ServerCapabilities {
    let tools = if input.has_tools {
        Some(ToolsCapabilities {
            list_changed: Some(input.tools_list_changed),
        })
    } else {
        None
    };

    let prompts = if input.has_prompts {
        Some(PromptsCapabilities {
            list_changed: Some(input.prompts_list_changed),
        })
    } else {
        None
    };

    let resources = if input.has_resources {
        Some(ResourcesCapabilities {
            list_changed: Some(input.resources_list_changed),
            subscribe: Some(input.resources_subscribe),
        })
    } else {
        None
    };

    let logging = if input.has_logging {
        Some(LoggingCapabilities {})
    } else {
        None
    };

    let completions = if input.has_completions {
        Some(CompletionCapabilities {})
    } else {
        None
    };

    let experimental = if input.has_experimental && !input.experimental_keys.is_empty() {
        let mut map = std::collections::HashMap::new();
        for key in input.experimental_keys.iter().take(5) {
            // Limit key length
            let k: String = key.chars().take(32).collect();
            if !k.is_empty() {
                map.insert(k, serde_json::json!({"enabled": true}));
            }
        }
        if map.is_empty() {
            None
        } else {
            Some(map)
        }
    } else {
        None
    };

    ServerCapabilities {
        experimental,
        logging,
        completions,
        prompts,
        resources,
        tools,
    }
}

fn build_client_capabilities(input: &CapabilityFuzzInput) -> ClientCapabilities {
    let roots = if input.has_roots {
        Some(RootsCapabilities {
            list_changed: Some(input.roots_list_changed),
        })
    } else {
        None
    };

    let sampling = if input.has_sampling {
        Some(SamplingCapabilities {})
    } else {
        None
    };

    let elicitation = if input.has_elicitation {
        Some(ElicitationCapabilities {
            schema_validation: Some(input.elicitation_schema_validation),
        })
    } else {
        None
    };

    let experimental = if input.has_experimental && !input.experimental_keys.is_empty() {
        let mut map = std::collections::HashMap::new();
        for key in input.experimental_keys.iter().take(5) {
            let k: String = key.chars().take(32).collect();
            if !k.is_empty() {
                map.insert(k, serde_json::json!({"enabled": true}));
            }
        }
        if map.is_empty() {
            None
        } else {
            Some(map)
        }
    } else {
        None
    };

    ClientCapabilities {
        roots,
        sampling,
        elicitation,
        experimental,
    }
}
