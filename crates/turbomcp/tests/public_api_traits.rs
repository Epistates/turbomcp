//! Trait obligations the public surface owes its users, asserted at compile
//! time rather than left to review.
//!
//! Two of the Rust API Guidelines' "eagerly implemented" traits are the ones
//! that actually break callers when missing, and both fail silently until
//! someone downstream hits them:
//!
//! - **`Error + Send + Sync + 'static`** (C-GOOD-ERR) is the bound `anyhow`,
//!   `eyre`, `Box<dyn Error + Send + Sync>`, and every `?` across a thread
//!   boundary require. An error type missing `Sync` compiles fine here and
//!   fails in the first application that spawns.
//! - **`Debug`** (C-COMMON-TRAITS) is required transitively: a user who puts
//!   one of our types in their own struct cannot `#[derive(Debug)]` without it,
//!   and the error names *their* type, not ours.
//!
//! These are `const` assertions, so the cost is compile-time only.

#![allow(dead_code)]

const fn assert_error<E: std::error::Error + Send + Sync + 'static>() {}
const fn assert_debug<T: std::fmt::Debug>() {}

#[test]
fn public_error_types_satisfy_the_error_bound() {
    assert_error::<turbomcp::McpError>();
    assert_error::<turbomcp::CodecError>();
    assert_error::<turbomcp::ProtocolError>();
    assert_error::<turbomcp_transport_stdio::StdioError>();
    #[cfg(feature = "client")]
    {
        assert_error::<turbomcp::client::ClientError>();
        assert_error::<turbomcp_client::HttpClientError>();
    }
    #[cfg(feature = "http")]
    assert_error::<turbomcp::http::HttpError>();
    #[cfg(feature = "websocket")]
    assert_error::<turbomcp_transport_ws::WsError>();
    #[cfg(feature = "auth")]
    assert_error::<turbomcp_auth::AuthError>();
    #[cfg(feature = "client-oauth")]
    assert_error::<turbomcp::auth::client::OAuthClientError>();
}

/// The types a user is most likely to hold in a struct of their own. Every one
/// of these lacked `Debug` until the API-guidelines pass, which meant a field
/// of this type silently poisoned the containing struct's derive.
#[test]
fn types_users_hold_are_debug() {
    assert_debug::<turbomcp::McpError>();
    assert_debug::<turbomcp_core::Identity>();
    assert_debug::<turbomcp_core::RequestContext>();
    assert_debug::<turbomcp_server::ServerNotifier>();
    assert_debug::<turbomcp_server::SessionStore>();
    assert_debug::<turbomcp_server::TaskStore>();
    assert_debug::<turbomcp_service::outbound::WriterGuard>();
    assert_debug::<turbomcp_transport_stdio::StdioTransport>();

    #[cfg(feature = "client")]
    {
        assert_debug::<turbomcp::client::Client>();
        assert_debug::<turbomcp::client::ClientBuilder>();
        assert_debug::<turbomcp_client::Connection>();
        assert_debug::<turbomcp_client::HttpClientTransport>();
    }
    #[cfg(feature = "ext-tasks")]
    assert_debug::<turbomcp_ext_tasks::TasksExtension>();
}

/// Generic wrappers must be `Debug` *without* bounding their parameter. A
/// `#[derive(Debug)]` on these would demand `S: Debug` of the user's own server
/// type, which is exactly the transitive breakage this file exists to catch —
/// so these assertions instantiate them with a deliberately non-`Debug` type.
#[test]
fn generic_wrappers_are_debug_without_bounding_their_parameter() {
    struct NotDebug;

    assert_debug::<turbomcp_server::MethodRouter<NotDebug>>();
    assert_debug::<turbomcp_server::ServerBuilder<NotDebug>>();
    assert_debug::<turbomcp_server::VersionDispatcher<NotDebug>>();
    assert_debug::<turbomcp_server::LegacySessionAdapter<NotDebug>>();
    assert_debug::<turbomcp_transport_stdio::LineTransport<NotDebug, NotDebug, NotDebug>>();
}
