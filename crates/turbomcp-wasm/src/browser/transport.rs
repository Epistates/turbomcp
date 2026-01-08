//! Browser transport implementations using Fetch API and WebSocket API

use serde::{Serialize, de::DeserializeOwned};
use std::cell::RefCell;
use std::rc::Rc;
use turbomcp_core::error::McpError;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AbortController, Headers, MessageEvent, Request, RequestInit, RequestMode, Response, WebSocket,
};

/// HTTP transport using the Fetch API
#[derive(Clone)]
pub struct FetchTransport {
    base_url: String,
    headers: Vec<(String, String)>,
    timeout_ms: u32,
}

impl FetchTransport {
    /// Create a new Fetch transport
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            headers: Vec::new(),
            timeout_ms: 30_000,
        }
    }

    /// Add a header to all requests
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((key.into(), value.into()));
        self
    }

    /// Set request timeout in milliseconds
    pub fn with_timeout(mut self, timeout_ms: u32) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    /// Send a JSON-RPC request
    pub async fn request<T: Serialize, R: DeserializeOwned>(
        &self,
        method: &str,
        params: Option<T>,
    ) -> Result<R, McpError> {
        let url = format!("{}/{}", self.base_url, method);

        // Create request body
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        });

        let body_str = serde_json::to_string(&body)
            .map_err(|e| McpError::serialization(format!("Failed to serialize request: {e}")))?;

        // Create abort controller for timeout
        let abort_controller = AbortController::new()
            .map_err(|e| McpError::transport(format!("Failed to create AbortController: {e:?}")))?;

        // Set up timeout
        let window =
            web_sys::window().ok_or_else(|| McpError::transport("No window object available"))?;
        let abort_signal = abort_controller.signal();

        let timeout_closure = Closure::once(Box::new(move || {
            abort_controller.abort();
        }) as Box<dyn FnOnce()>);

        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
            timeout_closure.as_ref().unchecked_ref(),
            self.timeout_ms as i32,
        );
        timeout_closure.forget();

        // Create headers
        let headers = Headers::new()
            .map_err(|e| McpError::transport(format!("Failed to create headers: {e:?}")))?;

        headers
            .set("Content-Type", "application/json")
            .map_err(|e| McpError::transport(format!("Failed to set Content-Type: {e:?}")))?;

        for (key, value) in &self.headers {
            headers
                .set(key, value)
                .map_err(|e| McpError::transport(format!("Failed to set header {key}: {e:?}")))?;
        }

        // Create request init
        let init = RequestInit::new();
        init.set_method("POST");
        init.set_headers(&headers);
        init.set_body(&JsValue::from_str(&body_str));
        init.set_mode(RequestMode::Cors);
        init.set_signal(Some(&abort_signal));

        // Create and send request
        let request = Request::new_with_str_and_init(&url, &init)
            .map_err(|e| McpError::transport(format!("Failed to create request: {e:?}")))?;

        let window =
            web_sys::window().ok_or_else(|| McpError::transport("No window object available"))?;

        let response: Response = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|e| {
                if abort_signal.aborted() {
                    McpError::timeout("Request timed out")
                } else {
                    McpError::transport(format!("Fetch failed: {e:?}"))
                }
            })?
            .dyn_into()
            .map_err(|e| McpError::transport(format!("Invalid response type: {e:?}")))?;

        if !response.ok() {
            return Err(McpError::transport(format!(
                "HTTP error: {} {}",
                response.status(),
                response.status_text()
            )));
        }

        // Parse response
        let text = JsFuture::from(
            response
                .text()
                .map_err(|e| McpError::transport(format!("Failed to get response text: {e:?}")))?,
        )
        .await
        .map_err(|e| McpError::transport(format!("Failed to read response: {e:?}")))?
        .as_string()
        .ok_or_else(|| McpError::transport("Response was not a string"))?;

        // Parse JSON-RPC response
        let rpc_response: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| McpError::parse_error(format!("Failed to parse response: {e}")))?;

        if let Some(error) = rpc_response.get("error") {
            let code = error.get("code").and_then(|c| c.as_i64()).unwrap_or(-32603) as i32;
            let message = error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            return Err(McpError::from_rpc_code(code, message));
        }

        let result = rpc_response
            .get("result")
            .ok_or_else(|| McpError::parse_error("No result in response"))?;

        serde_json::from_value(result.clone())
            .map_err(|e| McpError::parse_error(format!("Failed to parse result: {e}")))
    }
}

/// WebSocket transport for bidirectional MCP communication
pub struct WebSocketTransport {
    ws: WebSocket,
    message_handler: Rc<RefCell<Option<Box<dyn Fn(String)>>>>,
}

impl WebSocketTransport {
    /// Connect to a WebSocket endpoint
    pub async fn connect(url: &str) -> Result<Self, McpError> {
        let ws = WebSocket::new(url)
            .map_err(|e| McpError::transport(format!("Failed to create WebSocket: {e:?}")))?;

        ws.set_binary_type(web_sys::BinaryType::Arraybuffer);

        let message_handler: Rc<RefCell<Option<Box<dyn Fn(String)>>>> = Rc::new(RefCell::new(None));
        let handler_clone = message_handler.clone();

        // Set up message handler
        let onmessage = Closure::wrap(Box::new(move |e: MessageEvent| {
            if let Some(text) = e.data().as_string() {
                if let Some(ref handler) = *handler_clone.borrow() {
                    handler(text);
                }
            }
        }) as Box<dyn Fn(MessageEvent)>);

        ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        onmessage.forget();

        // Wait for connection
        let ws_clone = ws.clone();
        let (tx, rx) = futures_channel::oneshot::channel::<Result<(), McpError>>();
        let tx = Rc::new(RefCell::new(Some(tx)));

        let tx_open = tx.clone();
        let onopen = Closure::once(Box::new(move || {
            if let Some(tx) = tx_open.borrow_mut().take() {
                let _ = tx.send(Ok(()));
            }
        }) as Box<dyn FnOnce()>);

        let tx_error = tx;
        let onerror = Closure::once(Box::new(move |_: web_sys::ErrorEvent| {
            if let Some(tx) = tx_error.borrow_mut().take() {
                let _ = tx.send(Err(McpError::transport("WebSocket connection failed")));
            }
        }) as Box<dyn FnOnce(web_sys::ErrorEvent)>);

        ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
        ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));

        onopen.forget();
        onerror.forget();

        rx.await
            .map_err(|_| McpError::transport("Connection channel closed"))??;

        Ok(Self {
            ws: ws_clone,
            message_handler,
        })
    }

    /// Send a message
    pub fn send(&self, message: &str) -> Result<(), McpError> {
        self.ws
            .send_with_str(message)
            .map_err(|e| McpError::transport(format!("Failed to send message: {e:?}")))
    }

    /// Set message handler
    pub fn on_message(&self, handler: impl Fn(String) + 'static) {
        *self.message_handler.borrow_mut() = Some(Box::new(handler));
    }

    /// Close the connection
    pub fn close(&self) -> Result<(), McpError> {
        self.ws
            .close()
            .map_err(|e| McpError::transport(format!("Failed to close WebSocket: {e:?}")))
    }

    /// Check if connected
    pub fn is_connected(&self) -> bool {
        self.ws.ready_state() == WebSocket::OPEN
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch_transport_builder() {
        let transport = FetchTransport::new("https://api.example.com")
            .with_header("Authorization", "Bearer token")
            .with_timeout(60_000);

        assert_eq!(transport.base_url, "https://api.example.com");
        assert_eq!(transport.headers.len(), 1);
        assert_eq!(transport.timeout_ms, 60_000);
    }
}
