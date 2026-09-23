//! Web APIs reached through the JavaScript global object.
//!
//! `web_sys::window()` is `None` on Cloudflare Workers, Deno Deploy and in any
//! other worker context: their global object is a `WorkerGlobalScope`, not a
//! `Window`. Every server-side use of `crypto` and `fetch` went through
//! `window()` and so failed on exactly the platforms this module targets — JWT
//! verification, JWKS fetching, OAuth token hashing and request-id randomness
//! all returned "No window object available". Both APIs live on the global
//! object in every JS environment, so they are looked up there instead.

use wasm_bindgen::{JsCast, JsValue};

/// The global object: `globalThis` in any JS environment.
fn property(name: &str) -> Option<JsValue> {
    js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str(name))
        .ok()
        .filter(|value| !value.is_undefined() && !value.is_null())
}

/// The Web Crypto object (`globalThis.crypto`).
pub(crate) fn crypto() -> Option<web_sys::Crypto> {
    property("crypto").map(JsCast::unchecked_into)
}

/// `globalThis.fetch(request)`.
pub(crate) fn fetch(request: &web_sys::Request) -> Result<js_sys::Promise, JsValue> {
    let fetch: js_sys::Function = property("fetch")
        .ok_or_else(|| JsValue::from_str("fetch is not available in this environment"))?
        .unchecked_into();
    fetch
        .call1(&js_sys::global(), request)?
        .dyn_into::<js_sys::Promise>()
}
