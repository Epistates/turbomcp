//! STDIO transport for WASI MCP clients
//!
//! This transport uses WASI Preview 2's `wasi:cli/stdin` and `wasi:cli/stdout`
//! interfaces for JSON-RPC communication with MCP servers.
//!
//! # Protocol
//!
//! Messages are sent as newline-delimited JSON (NDJSON):
//! - Each JSON-RPC message is a single line
//! - Lines are terminated with `\n`
//! - The transport reads complete lines from stdin and writes complete lines to stdout

use super::transport::{
    JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, Transport, TransportError,
};
use serde::{Serialize, de::DeserializeOwned};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// STDIO transport for WASI environments
///
/// Uses `wasi:cli/stdin` and `wasi:cli/stdout` for communication.
/// Thread-safe request ID generation ensures unique IDs across concurrent requests.
///
/// # Example
///
/// ```ignore
/// use turbomcp_wasm::wasi::StdioTransport;
///
/// let transport = StdioTransport::new();
///
/// // Send a request
/// let result: serde_json::Value = transport.request("tools/list", None::<()>)?;
/// ```
pub struct StdioTransport {
    /// Next request ID (atomic for thread safety)
    next_id: AtomicU64,
    /// Buffer for accumulating data read from stdin
    #[allow(dead_code)] // Used in WASI builds
    read_buffer: RefCell<String>,
    /// Leftover data from previous reads (data after a newline)
    #[allow(dead_code)] // Used in WASI builds
    leftover: RefCell<String>,
    /// Whether the transport is open
    is_open: AtomicBool,
}

impl StdioTransport {
    /// Create a new STDIO transport
    #[must_use]
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            read_buffer: RefCell::new(String::with_capacity(4096)),
            leftover: RefCell::new(String::new()),
            is_open: AtomicBool::new(true),
        }
    }

    /// Get the next request ID
    fn next_request_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Write a message to stdout using WASI
    fn write_message(&self, message: &str) -> Result<(), TransportError> {
        #[cfg(target_os = "wasi")]
        {
            use wasi::cli::stdout::get_stdout;

            let stdout = get_stdout();
            let mut data = message.as_bytes().to_vec();
            data.push(b'\n');

            stdout
                .blocking_write_and_flush(&data)
                .map_err(|e| TransportError::Io(format!("Failed to write to stdout: {e:?}")))?;
        }

        #[cfg(not(target_os = "wasi"))]
        {
            // Fallback for non-WASI builds (testing)
            use std::io::Write;
            let mut stdout = std::io::stdout().lock();
            writeln!(stdout, "{message}")
                .map_err(|e| TransportError::Io(format!("Failed to write to stdout: {e}")))?;
            stdout
                .flush()
                .map_err(|e| TransportError::Io(format!("Failed to flush stdout: {e}")))?;
        }

        Ok(())
    }

    /// Read a line from stdin using WASI
    ///
    /// This method properly handles partial reads and preserves any data
    /// after the newline for subsequent reads (important for pipelined messages).
    fn read_line(&self) -> Result<String, TransportError> {
        #[cfg(target_os = "wasi")]
        {
            use wasi::cli::stdin::get_stdin;

            let stdin = get_stdin();
            let mut buffer = self.read_buffer.borrow_mut();
            let mut leftover = self.leftover.borrow_mut();

            // Start with any leftover data from previous reads
            buffer.clear();
            buffer.push_str(&leftover);
            leftover.clear();

            // Check if we already have a complete line in the leftover
            if let Some(newline_pos) = buffer.find('\n') {
                let line = buffer[..newline_pos].to_string();
                // Save everything after the newline for next read
                if newline_pos + 1 < buffer.len() {
                    leftover.push_str(&buffer[newline_pos + 1..]);
                }
                return Ok(line);
            }

            // Read until we get a newline
            loop {
                // Read in chunks
                let chunk = stdin
                    .blocking_read(4096)
                    .map_err(|e| TransportError::Io(format!("Failed to read from stdin: {e:?}")))?;

                if chunk.is_empty() {
                    if buffer.is_empty() {
                        return Err(TransportError::Io("EOF on stdin".to_string()));
                    }
                    // Return what we have if we hit EOF (no trailing newline)
                    return Ok(std::mem::take(&mut *buffer));
                }

                let text = String::from_utf8(chunk)
                    .map_err(|e| TransportError::Io(format!("Invalid UTF-8 in stdin: {e}")))?;

                if let Some(newline_pos) = text.find('\n') {
                    // Found a newline - add the part before it
                    buffer.push_str(&text[..newline_pos]);
                    // Save everything after the newline for next read
                    if newline_pos + 1 < text.len() {
                        leftover.push_str(&text[newline_pos + 1..]);
                    }
                    return Ok(std::mem::take(&mut *buffer));
                } else {
                    // No newline yet, add to buffer and continue
                    buffer.push_str(&text);
                }
            }
        }

        #[cfg(not(target_os = "wasi"))]
        {
            // Fallback for non-WASI builds (testing)
            use std::io::BufRead;
            let stdin = std::io::stdin();
            let mut line = String::new();
            stdin
                .lock()
                .read_line(&mut line)
                .map_err(|e| TransportError::Io(format!("Failed to read from stdin: {e}")))?;
            Ok(line.trim_end().to_string())
        }
    }
}

impl Default for StdioTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl Transport for StdioTransport {
    fn request<P, R>(&self, method: &str, params: Option<P>) -> Result<R, TransportError>
    where
        P: Serialize,
        R: DeserializeOwned,
    {
        if !self.is_open.load(Ordering::SeqCst) {
            return Err(TransportError::Connection(
                "Transport is closed".to_string(),
            ));
        }

        // Create request with unique ID
        let id = self.next_request_id();
        let request = JsonRpcRequest::new(id, method, params);

        // Serialize and send
        let request_json = serde_json::to_string(&request)?;
        self.write_message(&request_json)?;

        // Read response
        let response_json = self.read_line()?;
        let response: JsonRpcResponse<R> = serde_json::from_str(&response_json)?;

        // Check response ID matches (optional but good practice)
        if response.id != Some(id) {
            return Err(TransportError::Protocol(format!(
                "Response ID mismatch: expected {id}, got {:?}",
                response.id
            )));
        }

        response.into_result()
    }

    fn notify<P>(&self, method: &str, params: Option<P>) -> Result<(), TransportError>
    where
        P: Serialize,
    {
        if !self.is_open.load(Ordering::SeqCst) {
            return Err(TransportError::Connection(
                "Transport is closed".to_string(),
            ));
        }

        let notification = JsonRpcNotification::new(method, params);
        let json = serde_json::to_string(&notification)?;
        self.write_message(&json)
    }

    fn is_ready(&self) -> bool {
        self.is_open.load(Ordering::SeqCst)
    }

    fn close(&self) -> Result<(), TransportError> {
        self.is_open.store(false, Ordering::SeqCst);
        Ok(())
    }
}

// Note: StdioTransport is designed for single-threaded WASM environments
// Send/Sync are not implemented due to RefCell usage, but this is fine
// since WASM is single-threaded

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stdio_transport_creation() {
        let transport = StdioTransport::new();
        assert!(transport.is_ready());
    }

    #[test]
    fn test_stdio_transport_close() {
        let transport = StdioTransport::new();
        assert!(transport.is_ready());
        transport.close().unwrap();
        assert!(!transport.is_ready());
    }

    #[test]
    fn test_request_id_increment() {
        let transport = StdioTransport::new();
        assert_eq!(transport.next_request_id(), 1);
        assert_eq!(transport.next_request_id(), 2);
        assert_eq!(transport.next_request_id(), 3);
    }
}
