//! Server-Sent Events (SSE) encoding and decoding.
//!
//! This module provides pure, no-I/O SSE implementation for the MCP Streamable HTTP transport.
//!
//! ## SSE Format
//!
//! SSE messages consist of fields separated by newlines:
//! ```text
//! id: event-123
//! event: message
//! data: {"jsonrpc": "2.0", ...}
//!
//! ```
//!
//! Note: Messages are terminated by a blank line (two newlines).

#[cfg(not(feature = "std"))]
use alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
#[cfg(feature = "std")]
use std::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};

/// A Server-Sent Event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SseEvent {
    /// Event ID for resumption support
    pub id: Option<String>,
    /// Event type (defaults to "message" if not specified)
    pub event: Option<String>,
    /// Event data (can be multiline)
    pub data: String,
    /// Retry interval in milliseconds (optional)
    pub retry: Option<u32>,
}

impl SseEvent {
    /// Create a new SSE event with just data.
    pub fn message(data: impl Into<String>) -> Self {
        Self {
            id: None,
            event: None,
            data: data.into(),
            retry: None,
        }
    }

    /// Create a new SSE event with ID and data.
    pub fn with_id(id: impl Into<String>, data: impl Into<String>) -> Self {
        Self {
            id: Some(id.into()),
            event: None,
            data: data.into(),
            retry: None,
        }
    }

    /// Create a builder for more complex events.
    pub fn builder() -> SseEventBuilder {
        SseEventBuilder::new()
    }
}

/// Builder for constructing SSE events.
#[derive(Default)]
pub struct SseEventBuilder {
    id: Option<String>,
    event: Option<String>,
    data: Option<String>,
    retry: Option<u32>,
}

impl SseEventBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the event ID.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Set the event type.
    pub fn event(mut self, event: impl Into<String>) -> Self {
        self.event = Some(event.into());
        self
    }

    /// Set the event data.
    pub fn data(mut self, data: impl Into<String>) -> Self {
        self.data = Some(data.into());
        self
    }

    /// Set the retry interval in milliseconds.
    pub fn retry(mut self, retry_ms: u32) -> Self {
        self.retry = Some(retry_ms);
        self
    }

    /// Build the SSE event.
    ///
    /// # Panics
    ///
    /// Panics if data is not set.
    pub fn build(self) -> SseEvent {
        SseEvent {
            id: self.id,
            event: self.event,
            data: self.data.expect("SseEvent requires data"),
            retry: self.retry,
        }
    }

    /// Try to build the SSE event.
    ///
    /// Returns `None` if data is not set.
    pub fn try_build(self) -> Option<SseEvent> {
        Some(SseEvent {
            id: self.id,
            event: self.event,
            data: self.data?,
            retry: self.retry,
        })
    }
}

/// SSE encoder for converting events to wire format.
pub struct SseEncoder;

impl SseEncoder {
    /// Encode an SSE event to bytes.
    ///
    /// The output format follows the SSE specification:
    /// ```text
    /// id: <id>
    /// event: <type>
    /// retry: <ms>
    /// data: <line1>
    /// data: <line2>
    ///
    /// ```
    pub fn encode(event: &SseEvent) -> Vec<u8> {
        let mut output = String::new();

        // ID field (optional)
        if let Some(ref id) = event.id {
            output.push_str("id: ");
            output.push_str(id);
            output.push('\n');
        }

        // Event type field (optional)
        if let Some(ref event_type) = event.event {
            output.push_str("event: ");
            output.push_str(event_type);
            output.push('\n');
        }

        // Retry field (optional)
        if let Some(retry) = event.retry {
            output.push_str("retry: ");
            output.push_str(&retry.to_string());
            output.push('\n');
        }

        // Data field (required, can be multiline)
        for line in event.data.lines() {
            output.push_str("data: ");
            output.push_str(line);
            output.push('\n');
        }

        // Empty line to terminate the event
        output.push('\n');

        output.into_bytes()
    }

    /// Encode an SSE event to a string.
    pub fn encode_string(event: &SseEvent) -> String {
        // SAFETY: encode() only produces valid UTF-8
        String::from_utf8(Self::encode(event)).expect("SSE encoding produces valid UTF-8")
    }

    /// Encode a comment (used for keepalive).
    ///
    /// Comments start with `:` and are ignored by clients but keep the connection alive.
    pub fn encode_comment(comment: &str) -> Vec<u8> {
        let mut output = String::new();
        for line in comment.lines() {
            output.push_str(": ");
            output.push_str(line);
            output.push('\n');
        }
        output.push('\n');
        output.into_bytes()
    }

    /// Encode a keepalive ping (empty comment).
    pub fn encode_keepalive() -> Vec<u8> {
        b":\n\n".to_vec()
    }
}

/// SSE parser for decoding events from wire format.
pub struct SseParser {
    buffer: String,
    current_id: Option<String>,
    current_event: Option<String>,
    current_data: Vec<String>,
    current_retry: Option<u32>,
}

impl SseParser {
    /// Create a new SSE parser.
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            current_id: None,
            current_event: None,
            current_data: Vec::new(),
            current_retry: None,
        }
    }

    /// Feed data to the parser and extract any complete events.
    pub fn feed(&mut self, data: &[u8]) -> Vec<SseEvent> {
        // Append new data to buffer
        if let Ok(s) = core::str::from_utf8(data) {
            self.buffer.push_str(s);
        } else {
            // Invalid UTF-8, skip
            return vec![];
        }

        let mut events = Vec::new();

        // Process complete lines
        while let Some(newline_pos) = self.buffer.find('\n') {
            let line = self.buffer[..newline_pos].to_string();
            self.buffer = self.buffer[newline_pos + 1..].to_string();

            // Handle the line
            if line.is_empty() {
                // Empty line = end of event
                if let Some(event) = self.emit_event() {
                    events.push(event);
                }
            } else if line.starts_with(':') {
                // Comment, ignore
            } else if let Some(colon_pos) = line.find(':') {
                let field = &line[..colon_pos];
                let value = line[colon_pos + 1..].trim_start();

                match field {
                    "id" => self.current_id = Some(value.to_string()),
                    "event" => self.current_event = Some(value.to_string()),
                    "data" => self.current_data.push(value.to_string()),
                    "retry" => {
                        if let Ok(ms) = value.parse() {
                            self.current_retry = Some(ms);
                        }
                    }
                    _ => {} // Unknown field, ignore
                }
            } else {
                // Field with no value
                match line.as_str() {
                    "id" => self.current_id = Some(String::new()),
                    "event" => self.current_event = Some(String::new()),
                    "data" => self.current_data.push(String::new()),
                    _ => {}
                }
            }
        }

        events
    }

    /// Emit the current event if data is present.
    fn emit_event(&mut self) -> Option<SseEvent> {
        if self.current_data.is_empty() {
            // No data, clear state and return None
            self.current_id = None;
            self.current_event = None;
            self.current_retry = None;
            return None;
        }

        let data = self.current_data.join("\n");

        let event = SseEvent {
            id: self.current_id.take(),
            event: self.current_event.take(),
            data,
            retry: self.current_retry.take(),
        };

        self.current_data.clear();

        Some(event)
    }

    /// Reset the parser state.
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.current_id = None;
        self.current_event = None;
        self.current_data.clear();
        self.current_retry = None;
    }

    /// Get the last event ID seen.
    ///
    /// This is useful for reconnection with `Last-Event-ID` header.
    pub fn last_event_id(&self) -> Option<&str> {
        self.current_id.as_deref()
    }
}

impl Default for SseParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate a unique event ID.
///
/// Format: `{sequence}-{random}` where sequence is a monotonic counter
/// and random is entropy for uniqueness across instances.
#[cfg(feature = "std")]
pub fn generate_event_id(sequence: u64) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);

    // Simple ID: sequence + timestamp low bits for uniqueness
    format!("{sequence}-{:x}", timestamp & 0xFFFF_FFFF)
}

/// Generate an event ID with explicit timestamp (for no_std).
pub fn generate_event_id_with_timestamp(sequence: u64, timestamp_ns: u64) -> String {
    format!("{sequence}-{:x}", timestamp_ns & 0xFFFF_FFFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sse_event_message() {
        let event = SseEvent::message("Hello, world!");
        assert_eq!(event.data, "Hello, world!");
        assert!(event.id.is_none());
        assert!(event.event.is_none());
    }

    #[test]
    fn test_sse_event_with_id() {
        let event = SseEvent::with_id("123", "data");
        assert_eq!(event.id, Some("123".to_string()));
        assert_eq!(event.data, "data");
    }

    #[test]
    fn test_sse_event_builder() {
        let event = SseEvent::builder()
            .id("evt-1")
            .event("notification")
            .data(r#"{"type": "test"}"#)
            .retry(3000)
            .build();

        assert_eq!(event.id, Some("evt-1".to_string()));
        assert_eq!(event.event, Some("notification".to_string()));
        assert_eq!(event.data, r#"{"type": "test"}"#);
        assert_eq!(event.retry, Some(3000));
    }

    #[test]
    fn test_sse_encode_simple() {
        let event = SseEvent::message("hello");
        let encoded = SseEncoder::encode_string(&event);
        assert_eq!(encoded, "data: hello\n\n");
    }

    #[test]
    fn test_sse_encode_with_id() {
        let event = SseEvent::with_id("123", "data");
        let encoded = SseEncoder::encode_string(&event);
        assert_eq!(encoded, "id: 123\ndata: data\n\n");
    }

    #[test]
    fn test_sse_encode_full() {
        let event = SseEvent::builder()
            .id("evt-1")
            .event("update")
            .data("line1\nline2")
            .retry(5000)
            .build();

        let encoded = SseEncoder::encode_string(&event);
        assert_eq!(
            encoded,
            "id: evt-1\nevent: update\nretry: 5000\ndata: line1\ndata: line2\n\n"
        );
    }

    #[test]
    fn test_sse_encode_comment() {
        let encoded = SseEncoder::encode_comment("keepalive");
        assert_eq!(encoded, b": keepalive\n\n");
    }

    #[test]
    fn test_sse_encode_keepalive() {
        let encoded = SseEncoder::encode_keepalive();
        assert_eq!(encoded, b":\n\n");
    }

    #[test]
    fn test_sse_parser_simple() {
        let mut parser = SseParser::new();
        let events = parser.feed(b"data: hello\n\n");

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "hello");
    }

    #[test]
    fn test_sse_parser_with_id() {
        let mut parser = SseParser::new();
        let events = parser.feed(b"id: 123\ndata: test\n\n");

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, Some("123".to_string()));
        assert_eq!(events[0].data, "test");
    }

    #[test]
    fn test_sse_parser_multiline_data() {
        let mut parser = SseParser::new();
        let events = parser.feed(b"data: line1\ndata: line2\ndata: line3\n\n");

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "line1\nline2\nline3");
    }

    #[test]
    fn test_sse_parser_multiple_events() {
        let mut parser = SseParser::new();
        let events = parser.feed(b"data: first\n\ndata: second\n\n");

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].data, "first");
        assert_eq!(events[1].data, "second");
    }

    #[test]
    fn test_sse_parser_incremental() {
        let mut parser = SseParser::new();

        // Feed partial data
        let events1 = parser.feed(b"id: 1\n");
        assert!(events1.is_empty());

        let events2 = parser.feed(b"data: partial\n");
        assert!(events2.is_empty());

        // Complete the event
        let events3 = parser.feed(b"\n");
        assert_eq!(events3.len(), 1);
        assert_eq!(events3[0].id, Some("1".to_string()));
        assert_eq!(events3[0].data, "partial");
    }

    #[test]
    fn test_sse_parser_ignores_comments() {
        let mut parser = SseParser::new();
        let events = parser.feed(b": this is a comment\ndata: actual data\n\n");

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "actual data");
    }

    #[test]
    fn test_sse_parser_retry() {
        let mut parser = SseParser::new();
        let events = parser.feed(b"retry: 5000\ndata: test\n\n");

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].retry, Some(5000));
    }

    #[test]
    fn test_sse_roundtrip() {
        let original = SseEvent::builder()
            .id("round-trip-1")
            .event("test")
            .data("multiline\ndata\nhere")
            .retry(1000)
            .build();

        let encoded = SseEncoder::encode(&original);

        let mut parser = SseParser::new();
        let events = parser.feed(&encoded);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0], original);
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_generate_event_id() {
        let id1 = generate_event_id(1);
        let id2 = generate_event_id(2);

        assert!(id1.starts_with("1-"));
        assert!(id2.starts_with("2-"));
        assert_ne!(id1, id2);
    }
}
