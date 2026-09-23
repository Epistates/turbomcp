//! Browser transport implementations using Fetch API and WebSocket API

use serde::{Serialize, de::DeserializeOwned};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use turbomcp_core::error::McpError;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AbortController, Headers, MessageEvent, Request, RequestInit, RequestMode, Response, WebSocket,
};

/// Global atomic request ID counter for browser transports
/// Ensures unique IDs across concurrent requests
static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

/// Type alias for WebSocket message handler to reduce type complexity
type MessageHandler = Rc<RefCell<Option<Box<dyn Fn(String)>>>>;

/// HTTP transport using the Fetch API
///
/// Speaks Streamable HTTP: it accepts JSON or SSE answers, keeps the
/// `Mcp-Session-Id` the server issues at `initialize` and sends it — with the
/// negotiated `MCP-Protocol-Version` — on every later request.
#[derive(Clone)]
pub struct FetchTransport {
    base_url: String,
    headers: Vec<(String, String)>,
    timeout_ms: u32,
    session_id: Rc<RefCell<Option<String>>>,
    protocol_version: Rc<RefCell<Option<String>>>,
}

impl FetchTransport {
    /// Create a new Fetch transport
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            headers: Vec::new(),
            timeout_ms: 30_000,
            session_id: Rc::new(RefCell::new(None)),
            protocol_version: Rc::new(RefCell::new(None)),
        }
    }

    /// Record the protocol version negotiated at `initialize`, sent as
    /// `MCP-Protocol-Version` from then on.
    pub fn set_protocol_version(&self, version: impl Into<String>) {
        *self.protocol_version.borrow_mut() = Some(version.into());
    }

    /// The session id the server issued, if any.
    pub fn session_id(&self) -> Option<String> {
        self.session_id.borrow().clone()
    }

    /// Every header a POST carries: the protocol's own, then the caller's.
    fn request_headers(&self) -> Vec<(String, String)> {
        let mut headers = vec![
            ("Content-Type".to_string(), "application/json".to_string()),
            ("Accept".to_string(), crate::client_http::ACCEPT.to_string()),
        ];
        if let Some(session_id) = self.session_id.borrow().as_ref() {
            headers.push((
                crate::client_http::SESSION_ID_HEADER.to_string(),
                session_id.clone(),
            ));
        }
        if let Some(version) = self.protocol_version.borrow().as_ref() {
            headers.push((
                crate::client_http::PROTOCOL_VERSION_HEADER.to_string(),
                version.clone(),
            ));
        }
        headers.extend(self.headers.iter().cloned());
        headers
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
        // Create request body with unique ID
        let request_id = NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        let mut body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
        });
        if let Some(params) = params {
            body["params"] = serde_json::to_value(params).map_err(|e| {
                McpError::serialization(format!("Failed to serialize request: {e}"))
            })?;
        }

        let reply = self.post(&body).await?;
        let rpc_response = crate::client_http::response_message(
            reply.content_type.as_deref(),
            &reply.body,
            request_id,
        )
        .map_err(McpError::parse_error)?;

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

    /// Send a JSON-RPC notification.
    ///
    /// A notification carries no `id`, and the server answers it `202
    /// Accepted` with no body; there is nothing to wait for or parse.
    pub async fn notify<T: Serialize>(
        &self,
        method: &str,
        params: Option<T>,
    ) -> Result<(), McpError> {
        let mut body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
        });
        if let Some(params) = params {
            body["params"] = serde_json::to_value(params).map_err(|e| {
                McpError::serialization(format!("Failed to serialize notification: {e}"))
            })?;
        }
        self.post(&body).await.map(|_| ())
    }

    /// POST one JSON-RPC message and read the reply.
    async fn post(&self, message: &serde_json::Value) -> Result<HttpReply, McpError> {
        let body_str = serde_json::to_string(message)
            .map_err(|e| McpError::serialization(format!("Failed to serialize request: {e}")))?;

        let window =
            web_sys::window().ok_or_else(|| McpError::transport("No window object available"))?;

        // Create abort controller for timeout
        let abort_controller = AbortController::new()
            .map_err(|e| McpError::transport(format!("Failed to create AbortController: {e:?}")))?;
        let abort_signal = abort_controller.signal();

        // The closure stays owned here and the timer is cleared once the
        // fetch settles. It used to be `forget()`-ed, leaking one closure per
        // request for the life of the page.
        let timeout_closure = Closure::once(Box::new(move || {
            abort_controller.abort();
        }) as Box<dyn FnOnce()>);
        let timeout_handle = window
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                timeout_closure.as_ref().unchecked_ref(),
                self.timeout_ms as i32,
            )
            .map_err(|e| McpError::transport(format!("Failed to set timeout: {e:?}")))?;

        let headers = Headers::new()
            .map_err(|e| McpError::transport(format!("Failed to create headers: {e:?}")))?;
        for (key, value) in self.request_headers() {
            headers
                .set(&key, &value)
                .map_err(|e| McpError::transport(format!("Failed to set header {key}: {e:?}")))?;
        }

        let init = RequestInit::new();
        init.set_method("POST");
        init.set_headers(&headers);
        init.set_body(&JsValue::from_str(&body_str));
        init.set_mode(RequestMode::Cors);
        init.set_signal(Some(&abort_signal));

        // JSON-RPC is method-agnostic at the transport layer — the method
        // belongs in the body, not the URL path.
        let request = Request::new_with_str_and_init(&self.base_url, &init)
            .map_err(|e| McpError::transport(format!("Failed to create request: {e:?}")))?;

        let fetched = JsFuture::from(window.fetch_with_request(&request)).await;
        window.clear_timeout_with_handle(timeout_handle);
        drop(timeout_closure);

        let response: Response = fetched
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

        let response_headers = response.headers();
        if let Ok(Some(session_id)) = response_headers.get(crate::client_http::SESSION_ID_HEADER) {
            *self.session_id.borrow_mut() = Some(session_id);
        }
        let content_type = response_headers.get("Content-Type").ok().flatten();

        let body = JsFuture::from(
            response
                .text()
                .map_err(|e| McpError::transport(format!("Failed to get response text: {e:?}")))?,
        )
        .await
        .map_err(|e| McpError::transport(format!("Failed to read response: {e:?}")))?
        .as_string()
        .ok_or_else(|| McpError::transport("Response was not a string"))?;

        Ok(HttpReply { content_type, body })
    }
}

/// A POST's reply, as far as the JSON-RPC layer needs it.
struct HttpReply {
    content_type: Option<String>,
    body: String,
}

/// Type alias for WebSocket close handler
type CloseHandler = Rc<RefCell<Option<Box<dyn Fn(u16, String)>>>>;

/// WebSocket transport for bidirectional MCP communication
pub struct WebSocketTransport {
    ws: WebSocket,
    message_handler: MessageHandler,
    close_handler: CloseHandler,
}

impl WebSocketTransport {
    /// Connect to a WebSocket endpoint
    pub async fn connect(url: &str) -> Result<Self, McpError> {
        let ws = WebSocket::new(url)
            .map_err(|e| McpError::transport(format!("Failed to create WebSocket: {e:?}")))?;

        ws.set_binary_type(web_sys::BinaryType::Arraybuffer);

        let message_handler: MessageHandler = Rc::new(RefCell::new(None));
        let handler_clone = message_handler.clone();

        // Set up message handler
        let onmessage = Closure::wrap(Box::new(move |e: MessageEvent| {
            if let Some(text) = e.data().as_string()
                && let Some(ref handler) = *handler_clone.borrow()
            {
                handler(text);
            }
        }) as Box<dyn Fn(MessageEvent)>);

        ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        onmessage.forget();

        // Set up close handler
        let close_handler: CloseHandler = Rc::new(RefCell::new(None));
        let close_handler_clone = close_handler.clone();

        let onclose = Closure::wrap(Box::new(move |e: web_sys::CloseEvent| {
            if let Some(ref handler) = *close_handler_clone.borrow() {
                handler(e.code(), e.reason());
            }
        }) as Box<dyn Fn(web_sys::CloseEvent)>);

        ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
        onclose.forget();

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
            close_handler,
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

    /// Set close handler
    ///
    /// The handler receives the close code and reason string.
    /// Common close codes:
    /// - 1000: Normal closure
    /// - 1001: Going away (e.g., server shutting down)
    /// - 1006: Abnormal closure (connection lost)
    pub fn on_close(&self, handler: impl Fn(u16, String) + 'static) {
        *self.close_handler.borrow_mut() = Some(Box::new(handler));
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

    #[test]
    fn posts_carry_streamable_http_headers() {
        let transport = FetchTransport::new("https://api.example.com");
        let before: Vec<_> = transport
            .request_headers()
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(before, ["Content-Type", "Accept"]);

        *transport.session_id.borrow_mut() = Some("mcp-abc".into());
        transport.set_protocol_version("2025-06-18");
        let after = transport.request_headers();
        assert!(after.contains(&("Accept".into(), crate::client_http::ACCEPT.into())));
        assert!(after.contains(&("Mcp-Session-Id".into(), "mcp-abc".into())));
        assert!(after.contains(&("MCP-Protocol-Version".into(), "2025-06-18".into())));

        // Clones share the session, so the builder-style client keeps it.
        assert_eq!(transport.clone().session_id().as_deref(), Some("mcp-abc"));
    }
}
