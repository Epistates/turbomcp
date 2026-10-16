//! `ctx.client.elicit_as::<T>`: the form comes from `T`'s schema, flattened
//! to what a form can render, and the answer parses back into `T`, on both
//! the inline (`2025-*`) and the retried (`2026-07-28`) path.
#![cfg(feature = "client")]

use std::sync::{Arc, Mutex};

use serde_json::{Map, Value, json};
use turbomcp::client::{ClientBuilder, ConnectMode, ElicitationHandler, async_trait};
use turbomcp::neutral;
use turbomcp::prelude::*;

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
enum Room {
    Single,
    Double,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct Booking {
    /// Check-in date.
    #[schemars(extend("format" = "date"))]
    date: String,
    nights: u32,
    room: Option<Room>,
}

#[derive(Clone)]
struct Hotel;

#[server(name = "hotel", version = "1.0.0")]
impl Hotel {
    /// Book a room.
    #[tool]
    async fn book(&self, ctx: &CallToolContext) -> McpResult<String> {
        Ok(
            match ctx
                .client
                .elicit_as::<Booking>("booking", "Book a room")
                .await?
            {
                Elicited::Accepted(b) => {
                    let room = match b.room {
                        Some(Room::Single) => "single",
                        Some(Room::Double) => "double",
                        None => "any",
                    };
                    format!("{} for {} nights, {room}", b.date, b.nights)
                }
                Elicited::Declined => "declined".into(),
                _ => "cancelled".into(),
            },
        )
    }
}

/// Fills the form and keeps the schema it was shown.
#[derive(Clone, Default)]
struct Guest(Arc<Mutex<Option<Value>>>);

#[async_trait]
impl ElicitationHandler for Guest {
    async fn elicit(&self, request: neutral::ElicitParams) -> neutral::ElicitOutcome {
        *self.0.lock().unwrap() = Some(request.requested_schema.clone());
        let content = json!({ "date": "2026-11-02", "nights": 3, "room": "double" });
        neutral::ElicitOutcome::new(
            neutral::ElicitAction::Accept,
            content.as_object().unwrap().clone(),
        )
    }
}

#[tokio::test]
async fn a_typed_elicitation_round_trips() {
    for mode in [ConnectMode::Modern, ConnectMode::Legacy] {
        let guest = Guest::default();
        let client = turbomcp::testing::connect(
            Hotel.into_server(),
            ClientBuilder::new("t", "1")
                .with_connect_mode(mode)
                .with_elicitation(guest.clone()),
        )
        .await
        .unwrap();
        let booked = client.call_tool("book", Map::new()).await.unwrap();
        assert_eq!(
            booked.text_content().as_deref(),
            Some("2026-11-02 for 3 nights, double"),
            "{mode:?}"
        );
        let schema = guest.0.lock().unwrap().clone().unwrap();
        assert!(
            schema.get("$schema").is_none() && schema.get("$defs").is_none(),
            "{schema}"
        );
        let props = &schema["properties"];
        assert_eq!(props["date"]["format"], "date");
        assert_eq!(props["nights"]["type"], "integer");
        assert!(props["nights"].get("format").is_none(), "{schema}");
        assert_eq!(props["room"]["type"], "string", "{schema}");
        assert_eq!(props["room"]["enum"], json!(["single", "double"]));
        let required: Vec<&str> = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert!(
            required.contains(&"date") && !required.contains(&"room"),
            "{schema}"
        );
    }
}
