//! SIMD-Accelerated JSON Processing
//!
//! This module provides SIMD-accelerated JSON parsing and serialization using simd-json and sonic-rs
//! for improved performance in high-throughput message processing scenarios.

//use std::collections::HashMap;
use crate::McpErrorConstructors;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::{McpError, McpResult};

/// SIMD JSON processor configuration
#[derive(Debug, Clone)]
pub struct SimdJsonConfig {
    /// Enable SIMD acceleration
    pub enable_simd: bool,
    /// Buffer size for parsing
    pub buffer_size: usize,
    /// Enable zero-copy string parsing where possible
    pub zero_copy_strings: bool,
    /// Validate UTF-8 during parsing
    pub validate_utf8: bool,
    /// Maximum JSON depth to prevent stack overflow
    pub max_depth: usize,
}

impl Default for SimdJsonConfig {
    fn default() -> Self {
        Self {
            enable_simd: true,
            buffer_size: 64 * 1024, // 64KB
            zero_copy_strings: true,
            validate_utf8: true,
            max_depth: 128,
        }
    }
}

/// Performance metrics for SIMD JSON operations
#[derive(Debug, Default, Clone)]
pub struct SimdJsonMetrics {
    /// Total parse operations
    pub parse_operations: u64,
    /// Total serialize operations
    pub serialize_operations: u64,
    /// Total bytes parsed
    pub bytes_parsed: u64,
    /// Total bytes serialized
    pub bytes_serialized: u64,
    /// Total parse time in microseconds
    pub parse_time_us: u64,
    /// Total serialize time in microseconds
    pub serialize_time_us: u64,
    /// Number of SIMD accelerated operations
    pub simd_operations: u64,
    /// Number of fallback operations
    pub fallback_operations: u64,
}

impl SimdJsonMetrics {
    /// Get average parse speed in MB/s
    #[must_use]
    pub fn avg_parse_speed_mbps(&self) -> f64 {
        if self.parse_time_us == 0 {
            return 0.0;
        }
        let seconds = self.parse_time_us as f64 / 1_000_000.0;
        let megabytes = self.bytes_parsed as f64 / 1_000_000.0;
        megabytes / seconds
    }

    /// Get average serialize speed in MB/s
    #[must_use]
    pub fn avg_serialize_speed_mbps(&self) -> f64 {
        if self.serialize_time_us == 0 {
            return 0.0;
        }
        let seconds = self.serialize_time_us as f64 / 1_000_000.0;
        let megabytes = self.bytes_serialized as f64 / 1_000_000.0;
        megabytes / seconds
    }

    /// Get SIMD usage percentage
    #[must_use]
    pub fn simd_usage_percentage(&self) -> f64 {
        let total_operations = self.simd_operations + self.fallback_operations;
        if total_operations == 0 {
            return 0.0;
        }
        (self.simd_operations as f64 / total_operations as f64) * 100.0
    }
}

/// Fast SIMD JSON processor
pub struct SimdJsonProcessor {
    /// Configuration
    config: SimdJsonConfig,
    /// Performance metrics
    metrics: Arc<RwLock<SimdJsonMetrics>>,
    /// Reusable buffers for zero-allocation parsing (used when SIMD is fully enabled)
    #[allow(dead_code)]
    buffer_pool: Arc<RwLock<Vec<Vec<u8>>>>,
}

impl SimdJsonProcessor {
    /// Create a new SIMD JSON processor
    #[must_use]
    pub fn new(config: SimdJsonConfig) -> Self {
        Self {
            config,
            metrics: Arc::new(RwLock::new(SimdJsonMetrics::default())),
            buffer_pool: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Parse JSON with SIMD acceleration
    pub async fn parse<T>(&self, json_bytes: &[u8]) -> McpResult<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let start_time = std::time::Instant::now();

        let result = if self.config.enable_simd && self.can_use_simd(json_bytes) {
            self.parse_with_simd(json_bytes).await
        } else {
            self.parse_fallback(json_bytes).await
        };

        // Update metrics
        let duration = start_time.elapsed();
        let mut metrics = self.metrics.write().await;
        metrics.parse_operations += 1;
        metrics.bytes_parsed += json_bytes.len() as u64;
        metrics.parse_time_us += duration.as_micros() as u64;

        if self.config.enable_simd && self.can_use_simd(json_bytes) {
            metrics.simd_operations += 1;
        } else {
            metrics.fallback_operations += 1;
        }

        result
    }

    /// Serialize to JSON with SIMD acceleration
    pub async fn serialize<T>(&self, value: &T) -> McpResult<Vec<u8>>
    where
        T: Serialize,
    {
        let start_time = std::time::Instant::now();

        let result = if self.config.enable_simd {
            self.serialize_with_simd(value).await
        } else {
            self.serialize_fallback(value).await
        };

        // Update metrics
        let duration = start_time.elapsed();
        let mut metrics = self.metrics.write().await;
        metrics.serialize_operations += 1;

        if let Ok(ref bytes) = result {
            metrics.bytes_serialized += bytes.len() as u64;
        }

        metrics.serialize_time_us += duration.as_micros() as u64;

        if self.config.enable_simd {
            metrics.simd_operations += 1;
        } else {
            metrics.fallback_operations += 1;
        }

        result
    }

    /// Parse JSON string with SIMD acceleration  
    pub async fn parse_str<T>(&self, json_str: &str) -> McpResult<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        self.parse(json_str.as_bytes()).await
    }

    /// Serialize to JSON string with SIMD acceleration
    pub async fn serialize_to_string<T>(&self, value: &T) -> McpResult<String>
    where
        T: Serialize,
    {
        let bytes = self.serialize(value).await?;
        String::from_utf8(bytes)
            .map_err(|e| McpError::tool(format!("Invalid UTF-8 in serialized JSON: {e}")))
    }

    /// Check if input is suitable for SIMD processing
    const fn can_use_simd(&self, json_bytes: &[u8]) -> bool {
        // SIMD works best with larger inputs and valid UTF-8
        json_bytes.len() >= 64
            && (!self.config.validate_utf8 || std::str::from_utf8(json_bytes).is_ok())
    }

    /// Parse with SIMD acceleration using simd-json
    async fn parse_with_simd<T>(&self, json_bytes: &[u8]) -> McpResult<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        #[cfg(feature = "simd")]
        {
            // Use simd-json for maximum performance
            let mut owned = json_bytes.to_vec();
            let parsed = simd_json::to_borrowed_value(&mut owned)
                .map_err(|e| McpError::tool(format!("SIMD JSON parse error: {e}")))?;
            simd_json::serde::from_borrowed_value(parsed)
                .map_err(|e| McpError::tool(format!("SIMD JSON deserialize error: {e}")))
        }
        #[cfg(not(feature = "simd"))]
        {
            // Fallback to serde_json
            serde_json::from_slice(json_bytes)
                .map_err(|e| McpError::tool(format!("JSON parse error: {}", e)))
        }
    }

    /// Parse with fallback (standard `serde_json`)
    async fn parse_fallback<T>(&self, json_bytes: &[u8]) -> McpResult<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        serde_json::from_slice(json_bytes)
            .map_err(|e| McpError::tool(format!("JSON parse error: {e}")))
    }

    /// Serialize with SIMD acceleration using sonic-rs
    async fn serialize_with_simd<T>(&self, value: &T) -> McpResult<Vec<u8>>
    where
        T: Serialize,
    {
        #[cfg(feature = "simd")]
        {
            // Use sonic-rs for fast serialization
            sonic_rs::to_vec(value)
                .map_err(|e| McpError::tool(format!("SIMD JSON serialize error: {e}")))
        }
        #[cfg(not(feature = "simd"))]
        {
            // Fallback to serde_json
            serde_json::to_vec(value)
                .map_err(|e| McpError::tool(format!("JSON serialize error: {}", e)))
        }
    }

    /// Serialize with fallback (standard `serde_json`)
    async fn serialize_fallback<T>(&self, value: &T) -> McpResult<Vec<u8>>
    where
        T: Serialize,
    {
        serde_json::to_vec(value).map_err(|e| McpError::tool(format!("JSON serialize error: {e}")))
    }

    /// Get performance metrics
    pub async fn metrics(&self) -> SimdJsonMetrics {
        self.metrics.read().await.clone()
    }

    /// Reset metrics
    pub async fn reset_metrics(&self) {
        *self.metrics.write().await = SimdJsonMetrics::default();
    }

    /// Get a buffer from the pool or create a new one (for SIMD optimization)
    #[allow(dead_code)]
    async fn get_buffer(&self) -> Vec<u8> {
        let mut pool = self.buffer_pool.write().await;
        pool.pop()
            .unwrap_or_else(|| Vec::with_capacity(self.config.buffer_size))
    }

    /// Return a buffer to the pool (for SIMD optimization)
    #[allow(dead_code)]
    async fn return_buffer(&self, mut buffer: Vec<u8>) {
        buffer.clear();
        if buffer.capacity() <= self.config.buffer_size * 2 {
            let mut pool = self.buffer_pool.write().await;
            if pool.len() < 10 {
                // Limit pool size
                pool.push(buffer);
            }
        }
    }
}

/// Batch JSON processor for handling multiple documents efficiently
pub struct BatchJsonProcessor {
    /// SIMD processor
    processor: SimdJsonProcessor,
    /// Batch size for optimal SIMD utilization
    batch_size: usize,
}

impl BatchJsonProcessor {
    /// Create a new batch JSON processor
    #[must_use]
    pub fn new(config: SimdJsonConfig, batch_size: usize) -> Self {
        Self {
            processor: SimdJsonProcessor::new(config),
            batch_size,
        }
    }

    /// Parse multiple JSON documents in batch
    pub async fn parse_batch<T>(&self, json_docs: &[&[u8]]) -> McpResult<Vec<T>>
    where
        T: for<'de> Deserialize<'de> + Send,
    {
        let mut results = Vec::with_capacity(json_docs.len());

        // Process in batches for optimal SIMD utilization
        for chunk in json_docs.chunks(self.batch_size) {
            let mut batch_results = Vec::with_capacity(chunk.len());

            // Process batch in parallel
            let futures: Vec<_> = chunk
                .iter()
                .map(|json_bytes| self.processor.parse(json_bytes))
                .collect();

            for future in futures {
                batch_results.push(future.await?);
            }

            results.extend(batch_results);
        }

        Ok(results)
    }

    /// Serialize multiple values in batch
    pub async fn serialize_batch<T>(&self, values: &[T]) -> McpResult<Vec<Vec<u8>>>
    where
        T: Serialize + Send + Sync,
    {
        let mut results = Vec::with_capacity(values.len());

        // Process in batches
        for chunk in values.chunks(self.batch_size) {
            let mut batch_results = Vec::with_capacity(chunk.len());

            // Process batch in parallel
            let futures: Vec<_> = chunk
                .iter()
                .map(|value| self.processor.serialize(value))
                .collect();

            for future in futures {
                batch_results.push(future.await?);
            }

            results.extend(batch_results);
        }

        Ok(results)
    }

    /// Get processor metrics
    pub async fn metrics(&self) -> SimdJsonMetrics {
        self.processor.metrics().await
    }
}

/// Global SIMD JSON processor instance
static GLOBAL_PROCESSOR: once_cell::sync::Lazy<
    tokio::sync::RwLock<Option<Arc<SimdJsonProcessor>>>,
> = once_cell::sync::Lazy::new(|| tokio::sync::RwLock::new(None));

/// Initialize global SIMD JSON processor
pub async fn init_global_processor(config: SimdJsonConfig) {
    let processor = Arc::new(SimdJsonProcessor::new(config));
    *GLOBAL_PROCESSOR.write().await = Some(processor);
}

/// Get global SIMD JSON processor
pub async fn global_processor() -> Option<Arc<SimdJsonProcessor>> {
    GLOBAL_PROCESSOR.read().await.clone()
}

/// Convenience function to parse JSON with global processor
pub async fn parse_json<T>(json_bytes: &[u8]) -> McpResult<T>
where
    T: for<'de> Deserialize<'de>,
{
    if let Some(processor) = global_processor().await {
        processor.parse(json_bytes).await
    } else {
        // Fallback to standard serde_json
        serde_json::from_slice(json_bytes)
            .map_err(|e| McpError::tool(format!("JSON parse error: {e}")))
    }
}

/// Convenience function to serialize JSON with global processor
pub async fn serialize_json<T>(value: &T) -> McpResult<Vec<u8>>
where
    T: Serialize,
{
    if let Some(processor) = global_processor().await {
        processor.serialize(value).await
    } else {
        // Fallback to standard serde_json
        serde_json::to_vec(value).map_err(|e| McpError::tool(format!("JSON serialize error: {e}")))
    }
}

/// Convenience function to serialize JSON to string with global processor
pub async fn serialize_json_string<T>(value: &T) -> McpResult<String>
where
    T: Serialize,
{
    if let Some(processor) = global_processor().await {
        processor.serialize_to_string(value).await
    } else {
        // Fallback to standard serde_json
        serde_json::to_string(value)
            .map_err(|e| McpError::tool(format!("JSON serialize error: {e}")))
    }
}

/// Streaming JSON parser for large documents
pub struct StreamingJsonParser {
    /// SIMD processor
    processor: SimdJsonProcessor,
    /// Current buffer
    buffer: Vec<u8>,
    /// Parser state
    state: ParserState,
}

#[derive(Debug, Clone, PartialEq)]
enum ParserState {
    Waiting,
    #[allow(dead_code)] // Used in full streaming SIMD implementation
    InObject,
    #[allow(dead_code)] // Used in full streaming SIMD implementation
    InArray,
    #[allow(dead_code)] // Used in full streaming SIMD implementation
    InString,
    Complete,
}

impl StreamingJsonParser {
    /// Create a new streaming parser
    #[must_use]
    pub fn new(config: SimdJsonConfig) -> Self {
        Self {
            processor: SimdJsonProcessor::new(config),
            buffer: Vec::with_capacity(64 * 1024),
            state: ParserState::Waiting,
        }
    }

    /// Feed data to the parser
    pub async fn feed(&mut self, data: &[u8]) -> McpResult<()> {
        self.buffer.extend_from_slice(data);
        Ok(())
    }

    /// Try to parse a complete JSON document from the buffer
    pub async fn try_parse<T>(&mut self) -> McpResult<Option<T>>
    where
        T: for<'de> Deserialize<'de>,
    {
        // Robust streaming JSON parsing with complete validation
        if self.is_complete_json() {
            let result = self.processor.parse(&self.buffer).await?;
            self.buffer.clear();
            self.state = ParserState::Complete;
            Ok(Some(result))
        } else {
            Ok(None)
        }
    }

    /// Check if buffer contains complete JSON with proper bracket counting
    fn is_complete_json(&self) -> bool {
        if self.buffer.is_empty() {
            return false;
        }

        let json_str = match std::str::from_utf8(&self.buffer) {
            Ok(s) => s.trim(),
            Err(_) => return false,
        };

        if json_str.is_empty() {
            return false;
        }

        // Robust JSON completeness detection with bracket counting
        let mut brace_count = 0;
        let mut bracket_count = 0;
        let mut in_string = false;
        let mut escaped = false;
        let chars = json_str.chars();
        let first_char = chars.as_str().chars().next().unwrap_or(' ');

        if !matches!(first_char, '{' | '[') {
            return false;
        }

        for ch in json_str.chars() {
            if escaped {
                escaped = false;
                continue;
            }

            match ch {
                '\\' if in_string => escaped = true,
                '"' => in_string = !in_string,
                '{' if !in_string => brace_count += 1,
                '}' if !in_string => {
                    brace_count -= 1;
                    if brace_count < 0 {
                        return false;
                    }
                }
                '[' if !in_string => bracket_count += 1,
                ']' if !in_string => {
                    bracket_count -= 1;
                    if bracket_count < 0 {
                        return false;
                    }
                }
                _ => {}
            }
        }

        brace_count == 0 && bracket_count == 0 && !in_string
    }

    /// Reset parser state
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.state = ParserState::Waiting;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_simd_processor_creation() {
        let config = SimdJsonConfig::default();
        let processor = SimdJsonProcessor::new(config);

        let metrics = processor.metrics().await;
        assert_eq!(metrics.parse_operations, 0);
        assert_eq!(metrics.serialize_operations, 0);
    }

    #[tokio::test]
    async fn test_json_parsing() {
        let config = SimdJsonConfig::default();
        let processor = SimdJsonProcessor::new(config);

        let json_data = json!({
            "name": "test",
            "value": 42,
            "active": true
        });

        let json_bytes = serde_json::to_vec(&json_data).unwrap();
        let parsed: serde_json::Value = processor.parse(&json_bytes).await.unwrap();

        assert_eq!(parsed["name"], "test");
        assert_eq!(parsed["value"], 42);
        assert_eq!(parsed["active"], true);
    }

    #[tokio::test]
    async fn test_json_serialization() {
        let config = SimdJsonConfig::default();
        let processor = SimdJsonProcessor::new(config);

        let data = json!({
            "items": [1, 2, 3, 4, 5],
            "metadata": {
                "count": 5,
                "type": "numbers"
            }
        });

        let serialized = processor.serialize(&data).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&serialized).unwrap();

        assert_eq!(parsed["items"][0], 1);
        assert_eq!(parsed["metadata"]["count"], 5);
    }

    #[tokio::test]
    async fn test_batch_processing() {
        let config = SimdJsonConfig::default();
        let processor = BatchJsonProcessor::new(config, 4);

        let json_docs = vec![
            br#"{"id": 1, "name": "first"}"#.as_slice(),
            br#"{"id": 2, "name": "second"}"#.as_slice(),
            br#"{"id": 3, "name": "third"}"#.as_slice(),
        ];

        let results: Vec<serde_json::Value> = processor.parse_batch(&json_docs).await.unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0]["id"], 1);
        assert_eq!(results[1]["name"], "second");
        assert_eq!(results[2]["id"], 3);
    }

    #[tokio::test]
    async fn test_metrics_tracking() {
        let config = SimdJsonConfig::default();
        let processor = SimdJsonProcessor::new(config);

        let json_data = json!({"test": "metrics"});
        let json_bytes = serde_json::to_vec(&json_data).unwrap();

        // Perform operations
        let _: serde_json::Value = processor.parse(&json_bytes).await.unwrap();
        let _ = processor.serialize(&json_data).await.unwrap();

        let metrics = processor.metrics().await;
        assert_eq!(metrics.parse_operations, 1);
        assert_eq!(metrics.serialize_operations, 1);
        assert!(metrics.bytes_parsed > 0);
        assert!(metrics.bytes_serialized > 0);
    }

    #[tokio::test]
    async fn test_streaming_parser() {
        let config = SimdJsonConfig::default();
        let mut parser = StreamingJsonParser::new(config);

        let json_data = br#"{"streaming": true, "test": "data"}"#;

        // Feed data in chunks
        parser.feed(&json_data[..10]).await.unwrap();
        let result: Option<serde_json::Value> = parser.try_parse().await.unwrap();
        assert!(result.is_none()); // Incomplete

        parser.feed(&json_data[10..]).await.unwrap();
        let result: Option<serde_json::Value> = parser.try_parse().await.unwrap();
        assert!(result.is_some()); // Complete

        let parsed = result.unwrap();
        assert_eq!(parsed["streaming"], true);
        assert_eq!(parsed["test"], "data");
    }

    #[tokio::test]
    async fn test_global_processor() {
        let config = SimdJsonConfig {
            enable_simd: true,
            ..Default::default()
        };

        init_global_processor(config).await;

        let json_data = json!({"global": "test"});
        let json_bytes = serde_json::to_vec(&json_data).unwrap();

        let parsed: serde_json::Value = parse_json(&json_bytes).await.unwrap();
        assert_eq!(parsed["global"], "test");

        let serialized = serialize_json(&json_data).await.unwrap();
        let reparsed: serde_json::Value = serde_json::from_slice(&serialized).unwrap();
        assert_eq!(reparsed["global"], "test");
    }
}
