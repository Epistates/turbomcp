//! Transport message types.

use std::collections::HashMap;
use std::time::Duration;

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use turbomcp_protocol::MessageId;

/// Maximum number of custom headers per message (DoS protection)
const MAX_CUSTOM_HEADERS: usize = 64;

/// A wrapper for a message being sent or received over a transport.
#[derive(Debug, Clone)]
pub struct TransportMessage {
    /// The unique identifier of the message.
    pub id: MessageId,

    /// The binary payload of the message.
    pub payload: Bytes,

    /// Metadata associated with the message.
    pub metadata: TransportMessageMetadata,
}

impl TransportMessage {
    /// Creates a new `TransportMessage` with a given ID and payload.
    pub fn new(id: MessageId, payload: Bytes) -> Self {
        Self {
            id,
            payload,
            metadata: TransportMessageMetadata::default(),
        }
    }

    /// Creates a new `TransportMessage` with the given ID, payload, and metadata.
    pub const fn with_metadata(
        id: MessageId,
        payload: Bytes,
        metadata: TransportMessageMetadata,
    ) -> Self {
        Self {
            id,
            payload,
            metadata,
        }
    }

    /// Returns the size of the message payload in bytes.
    pub const fn size(&self) -> usize {
        self.payload.len()
    }

    /// Returns `true` if the message is compressed.
    pub const fn is_compressed(&self) -> bool {
        self.metadata.encoding.is_some()
    }

    /// Returns the content type of the message, if specified.
    pub fn content_type(&self) -> Option<&str> {
        self.metadata.content_type.as_deref()
    }

    /// Returns the correlation ID of the message, if specified.
    pub fn correlation_id(&self) -> Option<&str> {
        self.metadata.correlation_id.as_deref()
    }
}

/// Metadata associated with a `TransportMessage`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransportMessageMetadata {
    /// The encoding of the message payload (e.g., "gzip").
    pub encoding: Option<String>,

    /// The MIME type of the message payload (e.g., "application/json").
    pub content_type: Option<String>,

    /// An ID used to correlate requests and responses.
    pub correlation_id: Option<String>,

    /// A map of custom headers.
    pub headers: HashMap<String, String>,

    /// The priority of the message (higher numbers indicate higher priority).
    pub priority: Option<u8>,

    /// The time-to-live for the message, in milliseconds.
    pub ttl: Option<u64>,

    /// A marker indicating that this is a heartbeat message.
    pub is_heartbeat: Option<bool>,
}

impl TransportMessageMetadata {
    /// Validate metadata constraints
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.headers.len() > MAX_CUSTOM_HEADERS {
            return Err("Too many custom headers");
        }
        Ok(())
    }

    /// Creates a new `TransportMessageMetadata` with a specified content type.
    pub fn with_content_type(content_type: impl Into<String>) -> Self {
        Self {
            content_type: Some(content_type.into()),
            ..Default::default()
        }
    }

    /// Creates a new `TransportMessageMetadata` with a specified correlation ID.
    pub fn with_correlation_id(correlation_id: impl Into<String>) -> Self {
        Self {
            correlation_id: Some(correlation_id.into()),
            ..Default::default()
        }
    }

    /// Adds a header to the metadata using a builder pattern.
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }

    /// Sets the priority of the message.
    #[must_use]
    pub const fn with_priority(mut self, priority: u8) -> Self {
        self.priority = Some(priority);
        self
    }

    /// Sets the time-to-live for the message.
    #[must_use]
    pub const fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = Some(ttl.as_millis() as u64);
        self
    }

    /// Marks the message as a heartbeat.
    #[must_use]
    pub const fn heartbeat(mut self) -> Self {
        self.is_heartbeat = Some(true);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transport_message_creation() {
        let id = MessageId::from("test");
        let payload = Bytes::from("test payload");
        let msg = TransportMessage::new(id.clone(), payload.clone());

        assert_eq!(msg.id, id);
        assert_eq!(msg.payload, payload);
        assert_eq!(msg.size(), 12);
    }

    #[test]
    fn test_transport_message_metadata() {
        let metadata = TransportMessageMetadata::default()
            .with_header("custom", "value")
            .with_priority(5)
            .with_ttl(Duration::from_secs(30));

        assert_eq!(metadata.headers.get("custom"), Some(&"value".to_string()));
        assert_eq!(metadata.priority, Some(5));
        assert_eq!(metadata.ttl, Some(30000));
    }

    #[test]
    fn test_metadata_header_limit() {
        let mut metadata = TransportMessageMetadata::default();

        // Add headers up to the limit
        for i in 0..MAX_CUSTOM_HEADERS {
            metadata
                .headers
                .insert(format!("key{}", i), format!("value{}", i));
        }
        assert!(metadata.validate().is_ok());

        // Exceed the limit
        metadata
            .headers
            .insert("overflow".to_string(), "value".to_string());
        assert!(metadata.validate().is_err());
    }
}
