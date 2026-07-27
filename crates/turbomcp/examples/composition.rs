//! # Composition (v4)
//!
//! Several focused servers, one endpoint. Each sub-server is an ordinary
//! `#[server]` impl that knows nothing about being mounted — `Composite` mounts
//! it under a prefix and namespaces what needs namespacing.
//!
//! - **Tools and prompts are prefixed** (`weather.forecast`, `news.headlines`).
//!   Their names are flat and short, chosen without knowing what else the
//!   process will serve, so prefixing is what makes collisions impossible.
//! - **Resource URIs are left alone.** A URI is already a namespace and a
//!   client may hand it elsewhere; rewriting it would make it a lie. Two mounts
//!   claiming one URI is reported rather than silently resolved.
//!
//! Run with: `cargo run -p turbomcp --example composition`

use turbomcp::prelude::*;
use turbomcp::{Composite, Implementation, LegacySessionAdapter, serve_stdio};

// ---- sub-server one ----------------------------------------------------------

#[derive(Clone)]
struct Weather;

#[server(name = "weather", version = "1.0.0")]
impl Weather {
    /// Tomorrow's forecast for a city.
    #[tool(tags("public"))]
    async fn forecast(&self, city: String) -> String {
        format!("{city}: sunny")
    }

    /// Note the scheme: a mounted server owning its own URI space is what lets
    /// resource URIs pass through composition unchanged.
    #[resource("weather://today", mime_type = "text/plain")]
    async fn today(&self) -> McpResult<String> {
        Ok("sunny".into())
    }

    #[prompt(description = "Explain a forecast in plain language")]
    async fn explain(&self, city: String) -> String {
        format!("Explain the weather in {city} to someone packing a bag.")
    }
}

// ---- sub-server two ----------------------------------------------------------

/// Declares a `forecast` tool too. Under composition both survive, as
/// `weather.forecast` and `news.forecast`.
#[derive(Clone)]
struct News;

#[server(name = "news", version = "1.0.0")]
impl News {
    /// What the papers say the weather will do.
    #[tool]
    async fn forecast(&self) -> String {
        "the papers say rain".into()
    }

    #[resource("news://today")]
    async fn today(&self) -> McpResult<String> {
        Ok("quiet".into())
    }
}

// ---- sub-server three: tools only --------------------------------------------

/// A mount need not implement everything: advertised capabilities are still
/// derived, so a composite of tools-only servers advertises tools alone.
#[derive(Clone)]
struct Health;

#[server(name = "health", version = "1.0.0")]
impl Health {
    #[tool(description = "Liveness check")]
    async fn ping(&self) -> String {
        "ok".into()
    }
}

#[tokio::main]
async fn main() -> McpResult<()> {
    // Logs MUST go to stderr — stdout carries the MCP protocol framing.
    let gateway = Composite::new(Implementation::new("gateway", "1.0.0"))
        .instructions("Weather lives under `weather.*`, headlines under `news.*`.")
        .mount("weather", Weather.into_server())?
        .mount("news", News.into_server())?
        .mount("health", Health.into_server())?;

    // `into_server()` registers exactly the capabilities the mounts provide,
    // then builds the dispatcher like any other server. `run_stdio()` is an
    // inherent method the `#[server]` macro generates, so a composite spells
    // the same thing out:
    let dispatcher = gateway.into_server().build();
    serve_stdio(LegacySessionAdapter::new(dispatcher))
        .await
        .map_err(|e| McpError::internal(e.to_string()))
}
