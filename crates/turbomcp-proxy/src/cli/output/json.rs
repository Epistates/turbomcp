//! JSON output formatter
//!
//! Provides JSON output (compact or pretty-printed) for scripting
//! and programmatic consumption.

use std::io::Write;

use super::OutputFormatter;
use crate::error::ProxyResult;
use crate::introspection::ServerSpec;

/// JSON formatter (compact or pretty)
pub struct JsonFormatter {
    pretty: bool,
}

impl JsonFormatter {
    /// Create a new JSON formatter
    #[must_use]
    pub fn new(pretty: bool) -> Self {
        Self { pretty }
    }
}

impl OutputFormatter for JsonFormatter {
    fn write_spec(&self, spec: &ServerSpec, writer: &mut dyn Write) -> ProxyResult<()> {
        let json = if self.pretty {
            serde_json::to_string_pretty(spec)?
        } else {
            serde_json::to_string(spec)?
        };

        writeln!(writer, "{json}")?;
        Ok(())
    }

    fn write_error(&self, error: &str, writer: &mut dyn Write) -> ProxyResult<()> {
        let error_obj = serde_json::json!({
            "error": error,
            "success": false
        });

        let json = if self.pretty {
            serde_json::to_string_pretty(&error_obj)?
        } else {
            serde_json::to_string(&error_obj)?
        };

        writeln!(writer, "{json}")?;
        Ok(())
    }

    fn write_success(&self, message: &str, writer: &mut dyn Write) -> ProxyResult<()> {
        let success_obj = serde_json::json!({
            "message": message,
            "success": true
        });

        let json = if self.pretty {
            serde_json::to_string_pretty(&success_obj)?
        } else {
            serde_json::to_string(&success_obj)?
        };

        writeln!(writer, "{json}")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::introspection::*;

    #[test]
    fn test_json_formatter() {
        let formatter = JsonFormatter::new(false);
        let spec = ServerSpec {
            server_info: ServerInfo {
                name: "test".to_string(),
                version: "1.0.0".to_string(),
                title: None,
            },
            protocol_version: "2025-06-18".to_string(),
            capabilities: ServerCapabilities::default(),
            tools: vec![],
            resources: vec![],
            resource_templates: vec![],
            prompts: vec![],
            instructions: None,
        };

        let mut output = Vec::new();
        formatter.write_spec(&spec, &mut output).unwrap();

        let json_str = String::from_utf8(output).unwrap();
        assert!(json_str.contains("test"));
        assert!(json_str.contains("1.0.0"));
    }
}
