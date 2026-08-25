//! Bucket-A A1: a `#[tool]` returning `Image` / `Audio` produces the matching
//! wire content block end-to-end (base64 `data` + `mimeType`), on the draft wire.

use serde_json::json;
use tower::{Service, ServiceExt};
use turbomcp::prelude::*;
use turbomcp::{Audio, Image, JsonRpcMessage, JsonRpcRequest};

#[derive(Clone)]
struct Media;

#[server(name = "media", version = "1.0.0")]
impl Media {
    /// Return a 1x1 PNG.
    #[tool(description = "Render an image")]
    async fn render(&self) -> Image {
        Image {
            data: "iVBORw0KGgo=".into(),
            mime_type: "image/png".into(),
        }
    }

    /// Return a short clip.
    #[tool(description = "Play a sound")]
    async fn sound(&self) -> McpResult<Audio> {
        Ok(Audio {
            data: "UklGRg==".into(),
            mime_type: "audio/wav".into(),
        })
    }
}

fn draft_meta() -> serde_json::Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
    })
}

async fn call(
    svc: &mut turbomcp::VersionDispatcher<Media>,
    req: JsonRpcRequest,
) -> serde_json::Value {
    let JsonRpcMessage::Response(r) = svc
        .ready()
        .await
        .unwrap()
        .call(req.into())
        .await
        .unwrap()
        .expect("a response")
    else {
        panic!("expected a response")
    };
    r.result.expect("a result")
}

#[tokio::test]
async fn image_and_audio_tools_return_wire_content_blocks() {
    let mut svc = Media.into_server().build();

    let img = call(
        &mut svc,
        JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({ "name": "render", "arguments": {}, "_meta": draft_meta() })),
        ),
    )
    .await;
    let block = &img["content"][0];
    assert_eq!(block["type"], "image", "got {img}");
    assert_eq!(block["data"], "iVBORw0KGgo=");
    assert_eq!(block["mimeType"], "image/png");
    assert_eq!(img["isError"], false);

    let audio = call(
        &mut svc,
        JsonRpcRequest::new(
            2,
            "tools/call",
            Some(json!({ "name": "sound", "arguments": {}, "_meta": draft_meta() })),
        ),
    )
    .await;
    let block = &audio["content"][0];
    assert_eq!(block["type"], "audio", "got {audio}");
    assert_eq!(block["data"], "UklGRg==");
    assert_eq!(block["mimeType"], "audio/wav");
}
