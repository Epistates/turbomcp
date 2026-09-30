//! # Composition (v4)
//!
//! Several focused servers, one endpoint. Each sub-server is an ordinary
//! `#[server]` impl that knows nothing about being mounted.
//!
//! This shows the shape most real servers want: a **flat core** plus
//! **prefixed plugins**.
//!
//! - **`mount_flat` renames nothing** (`ping` stays `ping`). That is what lets
//!   a large server be split into focused ones without breaking any client, so
//!   it is the right choice for a server's own components. The cost is that two
//!   flat mounts exposing one name is possible — so it is *detected*, and the
//!   list fails naming both servers rather than silently dropping one.
//! - **`mount` prefixes** (`weather__forecast`, `news__headlines`), which makes a
//!   collision impossible. The right choice for optional or third-party servers,
//!   where seeing which vertical a tool came from is a feature.
//! - **Resource URIs are left alone either way.** A URI is already a namespace
//!   and a client may hand it elsewhere; rewriting it would make it a lie. Two
//!   mounts claiming one URI is reported rather than silently resolved.
//!
//! Run with: `cargo run -p turbomcp --example composition`

use turbomcp::prelude::*;
use turbomcp::{Composite, Implementation, ProtocolVersion, RequestContext};

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
/// `weather__forecast` and `news__forecast`.
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

// ---- the core, mounted flat --------------------------------------------------

/// Mounted with `mount_flat`, so `ping` reaches clients as `ping`. A mount need
/// not implement everything either: advertised capabilities are still derived,
/// so a composite of tools-only servers advertises tools alone.
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
        .instructions("Weather lives under `weather__*`, headlines under `news__*`.")
        .mount("weather", Weather.into_server())?
        .mount("news", News.into_server())?
        // The core keeps its own names: this one serves `ping`, not `health.ping`.
        .mount_flat(Health.into_server())?;

    // Freeze it, then catch a name two mounts both claim *now* rather than on the
    // first client's `tools/list`. It checks the catalogue as this identity sees
    // it, so a server using `with_visibility` should preflight each identity that
    // matters.
    let composed = gateway.build();
    composed
        .preflight(RequestContext::new(ProtocolVersion::V2025_11_25))
        .await?;

    // `into_server()` registers exactly the capabilities the mounts provide,
    // and serves like any other server.
    composed
        .into_server()
        .serve(stdio())
        .await
        .map_err(|e| McpError::internal(e.to_string()))
}
