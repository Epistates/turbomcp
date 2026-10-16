//! `#[server]` beyond literals: a generic impl, attribute values that are
//! expressions (`env!`, `concat!`, a `const`), the crate's own name and
//! version when none is given, and a description from `include_str!`.

use std::marker::PhantomData;

use serde_json::Map;
use turbomcp::client::ClientBuilder;
use turbomcp::prelude::*;

const GREETING: &str = "Say hello";

/// Something a server is generic over.
trait Store: Clone + Send + Sync + 'static {
    fn get(&self) -> String;
}

#[derive(Clone)]
struct Memory;

impl Store for Memory {
    fn get(&self) -> String {
        "from memory".into()
    }
}

#[derive(Clone)]
struct App<S: Store> {
    store: S,
}

#[server(
    title = concat!("App ", "server"),
    instructions = GREETING,
)]
impl<S: Store> App<S> {
    /// Read the store.
    #[tool]
    async fn read(&self) -> String {
        self.store.get()
    }

    #[tool(description = GREETING, title = concat!("Hel", "lo"))]
    async fn hello(&self) -> String {
        "hi".into()
    }

    #[doc = include_str!("fixtures/report_tool.md")]
    #[tool]
    async fn report(&self) -> String {
        "report".into()
    }
}

#[derive(Clone)]
struct Phantom<T>(PhantomData<T>);

#[server(name = "phantom", version = env!("CARGO_PKG_VERSION"))]
impl<T> Phantom<T>
where
    T: Clone + Send + Sync + 'static,
{
    /// Nothing generic about it.
    #[tool]
    async fn ping(&self) -> String {
        "pong".into()
    }
}

#[tokio::test]
async fn a_generic_server_with_expression_attributes() {
    let client = turbomcp::testing::connect(
        App { store: Memory }.into_server(),
        ClientBuilder::new("t", "1"),
    )
    .await
    .unwrap();
    let info = client.server_info().unwrap();
    assert_eq!(
        info.name,
        env!("CARGO_PKG_NAME"),
        "the crate's name by default"
    );
    assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(info.title.as_deref(), Some("App server"));
    assert_eq!(client.instructions(), Some(GREETING));

    let tools = client.list_all_tools().await.unwrap();
    let tool = |name: &str| tools.iter().find(|t| t.name == name).unwrap().clone();
    assert_eq!(tool("hello").description.as_deref(), Some(GREETING));
    assert_eq!(tool("hello").title.as_deref(), Some("Hello"));
    assert_eq!(
        tool("report").description.as_deref(),
        Some("Build a report from the week's numbers."),
        "an included doc, reflowed, rustdoc-only sections dropped"
    );
    let read = client.call_tool("read", Map::new()).await.unwrap();
    assert_eq!(read.text_content().as_deref(), Some("from memory"));

    let phantom = turbomcp::testing::connect(
        Phantom::<u8>(PhantomData).into_server(),
        ClientBuilder::new("t", "1"),
    )
    .await
    .unwrap();
    assert_eq!(phantom.server_info().unwrap().name, "phantom");
}

// ---- inputs: Params<T>, typed arguments, names that used to shadow ----------

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Search {
    /// What to look for.
    query: String,
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct Brief {
    /// The topic.
    topic: String,
    tone: Option<String>,
}

#[derive(Clone)]
struct Inputs;

#[server(name = "inputs", version = "1.0.0")]
impl Inputs {
    /// Search with a shared argument type.
    #[tool]
    async fn search(&self, Params(search): Params<Search>) -> String {
        format!("{} x{}", search.query, search.limit.unwrap_or(10))
    }

    /// Repeat a word.
    #[prompt]
    async fn repeat(&self, word: String, times: u32, sep: Option<char>) -> String {
        let sep = sep.unwrap_or(' ').to_string();
        vec![word; times as usize].join(&sep)
    }

    /// Arguments named like the generated code's own.
    #[prompt]
    async fn shadow(&self, params: String, ctx: String) -> String {
        format!("{params}/{ctx}")
    }

    /// A brief from a whole argument type.
    #[prompt]
    async fn brief(&self, Params(brief): Params<Brief>) -> String {
        format!(
            "{} ({})",
            brief.topic,
            brief.tone.unwrap_or_else(|| "plain".into())
        )
    }

    #[resource("items://{id}")]
    async fn item(&self, id: u64) -> McpResult<String> {
        Ok(format!("item {id}"))
    }
}

#[tokio::test]
async fn whole_and_typed_inputs() {
    use std::collections::BTreeMap;
    let client = turbomcp::testing::connect(Inputs.into_server(), ClientBuilder::new("t", "1"))
        .await
        .unwrap();

    let tools = client.list_all_tools().await.unwrap();
    let schema = &tools[0].input_schema;
    assert!(schema["properties"]["query"].is_object(), "{schema}");
    assert_eq!(schema["required"], serde_json::json!(["query"]));
    let mut args = Map::new();
    args.insert("query".into(), "rust".into());
    args.insert("limit".into(), 3.into());
    let found = client.call_tool("search", args).await.unwrap();
    assert_eq!(found.text_content().as_deref(), Some("rust x3"));

    let get = |name: &'static str, pairs: &[(&str, &str)]| {
        let args: BTreeMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        client.get_prompt(name, args)
    };
    let text = |r: turbomcp::neutral::GetPromptResult| match &r.messages[0].content {
        turbomcp::neutral::Content::Text { text, .. } => text.clone(),
        other => panic!("{other:?}"),
    };
    let repeated = get("repeat", &[("word", "go"), ("times", "3"), ("sep", "-")])
        .await
        .unwrap();
    assert_eq!(text(repeated), "go-go-go");
    let bad = get("repeat", &[("word", "go"), ("times", "many")])
        .await
        .unwrap_err();
    assert!(bad.to_string().contains("times"), "{bad}");
    let shadow = get("shadow", &[("params", "p"), ("ctx", "c")])
        .await
        .unwrap();
    assert_eq!(text(shadow), "p/c");

    let prompts = client.list_all_prompts().await.unwrap();
    let brief = prompts.iter().find(|p| p.name == "brief").unwrap();
    let topic = brief.arguments.iter().find(|a| a.name == "topic").unwrap();
    assert!(topic.required);
    assert_eq!(topic.description.as_deref(), Some("The topic."));
    assert!(
        !brief
            .arguments
            .iter()
            .find(|a| a.name == "tone")
            .unwrap()
            .required
    );
    let made = get("brief", &[("topic", "MCP")]).await.unwrap();
    assert_eq!(text(made), "MCP (plain)");

    let item = client.read_resource("items://42").await.unwrap();
    assert!(
        matches!(&item.contents[0], turbomcp::neutral::ResourceContents::Text { text, .. } if text == "item 42"),
        "{item:?}"
    );
    let missing = client
        .read_resource("items://not-a-number")
        .await
        .unwrap_err();
    assert!(
        missing.to_string().to_lowercase().contains("not found"),
        "{missing}"
    );
}

// ---- metadata the attributes carry: icons, _meta, output schema, … ------------

#[derive(serde::Serialize, schemars::JsonSchema)]
struct Weather {
    celsius: f64,
}

#[derive(Clone)]
struct Decorated;

#[server(
    name = "decorated",
    version = "1.0.0",
    description = "Decorated on every surface",
    website_url = "https://example.com",
    icon("https://example.com/server.png")
)]
impl Decorated {
    /// Today's weather.
    #[tool(
        icon(src = "https://example.com/sun.svg", mime_type = "image/svg+xml", sizes("any"), theme = "dark"),
        meta("com.example/ui" = { "resourceUri": "ui://weather" }),
        output_schema = Weather,
    )]
    async fn weather(&self) -> turbomcp::neutral::CallToolResult {
        let mut result = turbomcp::neutral::CallToolResult::text("{\"celsius\":21.5}");
        result.structured_content = Some(serde_json::json!({ "celsius": 21.5 }));
        result
    }

    #[resource(
        "report://today",
        size = 1024,
        audience("user"),
        priority = 0.75,
        icon("https://example.com/report.png"),
        meta("com.example/kind" = "report"),
    )]
    async fn report(&self) -> String {
        "today".into()
    }

    /// Greet.
    #[prompt(icon("https://example.com/wave.png"))]
    async fn greet(&self, name: String) -> String {
        format!("Hello, {name}!")
    }
}

#[tokio::test]
async fn attributes_carry_icons_meta_and_schemas() {
    let client = turbomcp::testing::connect(
        Decorated.into_server(),
        ClientBuilder::new("t", "1").with_connect_mode(turbomcp::client::ConnectMode::Legacy),
    )
    .await
    .unwrap();
    let info = client.server_info().unwrap();
    assert_eq!(info.extra["description"], "Decorated on every surface");
    assert_eq!(info.extra["websiteUrl"], "https://example.com");
    assert_eq!(
        info.extra["icons"][0]["src"],
        "https://example.com/server.png"
    );

    let tool = client.list_all_tools().await.unwrap().remove(0);
    let icon = &tool.icons[0];
    assert_eq!(icon.mime_type.as_deref(), Some("image/svg+xml"));
    assert_eq!(icon.sizes, ["any"]);
    assert_eq!(icon.theme, Some(turbomcp::neutral::IconTheme::Dark));
    assert_eq!(tool.meta["com.example/ui"]["resourceUri"], "ui://weather");
    let output = tool
        .output_schema
        .as_ref()
        .expect("an explicit output schema");
    assert!(output["properties"]["celsius"].is_object(), "{output}");
    let called = client.call_tool("weather", Map::new()).await.unwrap();
    assert_eq!(called.structured_content.unwrap()["celsius"], 21.5);

    let resource = client.list_all_resources().await.unwrap().remove(0);
    assert_eq!(resource.size, Some(1024));
    let annotations = resource.annotations.unwrap();
    assert_eq!(annotations.audience, [turbomcp::neutral::Role::User]);
    assert_eq!(annotations.priority, Some(0.75));
    assert_eq!(resource.icons[0].src, "https://example.com/report.png");
    assert_eq!(resource.meta["com.example/kind"], "report");

    let prompt = client.list_all_prompts().await.unwrap().remove(0);
    assert_eq!(prompt.icons[0].src, "https://example.com/wave.png");
}
