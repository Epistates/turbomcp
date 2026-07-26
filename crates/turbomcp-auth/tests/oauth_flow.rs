//! The OAuth 2.1 client flow end-to-end against a mock authorization server:
//! discovery (well-known fallbacks + validation), dynamic registration with
//! `application_type`, PKCE (verified server-side), the RFC 8707 `resource`
//! parameter on both requests, RFC 9207 `iss` validation, token exchange,
//! refresh, and the issuer-keyed store. The "browser" is a redirect-following
//! HTTP GET.

#![cfg(feature = "oauth-client")]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::{Form, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect};
use axum::routing::{get, post};
use base64::Engine as _;
use serde_json::{Value, json};
use sha2::Digest as _;

use turbomcp_auth::client::{
    CallbackParams, DynamicRegistration, OAuthClient, RegistrationStrategy, parse_bearer_challenge,
};

/// What the mock AS observed + its per-test behavior switches.
#[derive(Default)]
struct MockState {
    registrations: Vec<Value>,
    authorize_queries: Vec<HashMap<String, String>>,
    token_forms: Vec<HashMap<String, String>>,
    /// code → (pkce challenge, granted scope)
    codes: HashMap<String, String>,
    /// Override the `iss` sent on the redirect (mix-up attack simulation).
    iss_override: Option<String>,
    /// Omit `code_challenge_methods_supported` from AS metadata.
    omit_pkce: bool,
    /// Lie about the issuer in the AS metadata document (impersonation).
    issuer_override: Option<String>,
    /// Redirect without `iss`, though the metadata advertises it (RFC 9207).
    omit_iss: bool,
    /// Redirect with `error`/`error_description` instead of a code (the user
    /// refused consent, or the AS rejected the request).
    deny: Option<(String, Option<String>)>,
    /// Answer a refresh with no new `refresh_token` (an AS that doesn't
    /// rotate).
    no_refresh_rotation: bool,
}

type Shared = Arc<Mutex<MockState>>;

async fn spawn_mock(state: Shared) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = axum::Router::new()
        .route(
            "/.well-known/oauth-protected-resource/mcp",
            get(resource_metadata),
        )
        .route("/.well-known/oauth-authorization-server", get(as_metadata))
        .route("/register", post(register))
        .route("/authorize", get(authorize))
        .route("/token", post(token))
        .with_state((state, base.clone()));
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    base
}

async fn resource_metadata(State((_, base)): State<(Shared, String)>) -> impl IntoResponse {
    axum::Json(json!({
        "resource": format!("{base}/mcp"),
        "authorization_servers": [base],
        "scopes_supported": ["mcp:tools"],
    }))
}

async fn as_metadata(State((state, base)): State<(Shared, String)>) -> impl IntoResponse {
    let s = state.lock().unwrap();
    let mut meta = json!({
        "issuer": s.issuer_override.clone().unwrap_or_else(|| base.clone()),
        "authorization_endpoint": format!("{base}/authorize"),
        "token_endpoint": format!("{base}/token"),
        "registration_endpoint": format!("{base}/register"),
        "authorization_response_iss_parameter_supported": true,
        "scopes_supported": ["mcp:tools", "files:write"],
    });
    if !s.omit_pkce {
        meta["code_challenge_methods_supported"] = json!(["S256"]);
    }
    axum::Json(meta)
}

async fn register(
    State((state, _)): State<(Shared, String)>,
    axum::Json(body): axum::Json<Value>,
) -> impl IntoResponse {
    state.lock().unwrap().registrations.push(body);
    axum::Json(json!({ "client_id": "dyn-client-1" }))
}

async fn authorize(
    State((state, base)): State<(Shared, String)>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let mut s = state.lock().unwrap();
    let challenge = params.get("code_challenge").cloned().unwrap_or_default();
    let redirect_uri = params.get("redirect_uri").cloned().unwrap_or_default();
    let req_state = params.get("state").cloned().unwrap_or_default();
    let code = format!("code-{}", s.codes.len() + 1);
    s.codes.insert(code.clone(), challenge);
    let iss = s.iss_override.clone().unwrap_or(base);
    let iss_param = if s.omit_iss {
        String::new()
    } else {
        format!("&iss={}", urlencode(&iss))
    };
    let deny = s.deny.clone();
    s.authorize_queries.push(params);
    if let Some((error, description)) = deny {
        // The AS refused: no code, an `error`, optionally a description.
        let described = description
            .map(|d| format!("&error_description={}", urlencode(&d)))
            .unwrap_or_default();
        return Redirect::to(&format!(
            "{redirect_uri}?error={}{described}&state={req_state}{iss_param}",
            urlencode(&error)
        ));
    }
    // The user "consents" instantly: redirect with code + state + iss.
    Redirect::to(&format!(
        "{redirect_uri}?code={code}&state={req_state}{iss_param}"
    ))
}

async fn token(
    State((state, _)): State<(Shared, String)>,
    Form(form): Form<HashMap<String, String>>,
) -> impl IntoResponse {
    let mut s = state.lock().unwrap();
    s.token_forms.push(form.clone());
    let grant = form.get("grant_type").map(String::as_str);
    if grant == Some("refresh_token") {
        if form.get("refresh_token").map(String::as_str) != Some("refresh-1") {
            return (
                StatusCode::BAD_REQUEST,
                axum::Json(json!({"error": "invalid_grant"})),
            )
                .into_response();
        }
        let mut body = json!({
            "access_token": "access-2",
            "token_type": "bearer",
            "expires_in": 3600,
        });
        if !s.no_refresh_rotation {
            body["refresh_token"] = json!("refresh-2");
        }
        return axum::Json(body).into_response();
    }
    // authorization_code: verify PKCE — S256(verifier) must equal the
    // challenge recorded at /authorize.
    let code = form.get("code").cloned().unwrap_or_default();
    let Some(challenge) = s.codes.remove(&code) else {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(json!({"error": "invalid_grant"})),
        )
            .into_response();
    };
    let verifier = form.get("code_verifier").cloned().unwrap_or_default();
    let hashed = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(sha2::Sha256::digest(verifier.as_bytes()));
    if hashed != challenge {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(json!({"error": "invalid_grant", "error_description": "pkce"})),
        )
            .into_response();
    }
    axum::Json(json!({
        "access_token": "access-1",
        "token_type": "bearer",
        "expires_in": 3600,
        "refresh_token": "refresh-1",
        "scope": "mcp:tools",
    }))
    .into_response()
}

fn urlencode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// Drive the "browser": GET the authorization URL without following the
/// redirect, and hand back the callback parameters from `Location`.
async fn browser(authorize_url: &str) -> CallbackParams {
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let resp = http.get(authorize_url).send().await.unwrap();
    assert_eq!(resp.status().as_u16(), 303, "mock consent redirects");
    let location = resp
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .expect("Location header");
    CallbackParams::from_query(location)
}

fn engine(base: &str) -> OAuthClient {
    OAuthClient::new(
        format!("{base}/mcp"),
        "http://127.0.0.1:19999/callback",
        RegistrationStrategy::Dynamic(DynamicRegistration::native(
            "turbomcp-test",
            vec!["http://127.0.0.1:19999/callback".to_owned()],
        )),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn full_flow_discovery_registration_pkce_exchange_refresh() {
    let state = Shared::default();
    let base = spawn_mock(Arc::clone(&state)).await;
    let engine = engine(&base);

    // Discovery from a parsed 401 challenge (the header names the metadata).
    let challenge = parse_bearer_challenge(&format!(
        "Bearer resource_metadata=\"{base}/.well-known/oauth-protected-resource/mcp\""
    ))
    .unwrap();
    let discovered = engine.discover(Some(&challenge)).await.unwrap();
    assert_eq!(discovered.server.issuer, base);

    // Registration: DCR fallback, with the mandatory application_type.
    let credentials = engine.credentials(&discovered).await.unwrap();
    assert_eq!(credentials.client_id, "dyn-client-1");
    {
        let s = state.lock().unwrap();
        assert_eq!(s.registrations.len(), 1);
        assert_eq!(s.registrations[0]["application_type"], "native");
        assert_eq!(s.registrations[0]["token_endpoint_auth_method"], "none");
    }

    // Scope selection: no challenge scope → resource scopes_supported.
    let scopes = OAuthClient::select_scopes(Some(&challenge), &discovered);
    assert_eq!(scopes, vec!["mcp:tools"]);

    // Authorize (PKCE + resource), "browser" consents, complete exchanges.
    let pending = engine.begin(&discovered, &credentials, &scopes).unwrap();
    let callback = browser(&pending.authorize_url).await;
    let tokens = engine
        .complete(&discovered, &credentials, pending, &callback)
        .await
        .unwrap();
    assert_eq!(tokens.access_token, "access-1");
    assert_eq!(tokens.refresh_token.as_deref(), Some("refresh-1"));
    assert_eq!(tokens.scopes, vec!["mcp:tools"]);

    // The RFC 8707 resource parameter reached BOTH requests.
    {
        let s = state.lock().unwrap();
        assert_eq!(
            s.authorize_queries[0].get("resource").map(String::as_str),
            Some(format!("{base}/mcp").as_str())
        );
        assert_eq!(
            s.token_forms[0].get("resource").map(String::as_str),
            Some(format!("{base}/mcp").as_str())
        );
        // And PKCE was real: challenge at authorize, verifier at token.
        assert!(s.authorize_queries[0].contains_key("code_challenge"));
        assert_eq!(
            s.authorize_queries[0]
                .get("code_challenge_method")
                .map(String::as_str),
            Some("S256")
        );
        assert!(s.token_forms[0].contains_key("code_verifier"));
    }

    // Refresh rotates and persists; resource parameter included again.
    let rotated = engine
        .refresh(&discovered, &credentials, &tokens)
        .await
        .unwrap();
    assert_eq!(rotated.access_token, "access-2");
    assert_eq!(rotated.refresh_token.as_deref(), Some("refresh-2"));
    {
        let s = state.lock().unwrap();
        let refresh_form = s.token_forms.last().unwrap();
        assert_eq!(
            refresh_form.get("grant_type").map(String::as_str),
            Some("refresh_token")
        );
        assert!(refresh_form.contains_key("resource"));
    }

    // The store is issuer-keyed: same issuer reuses the registration (no
    // second /register call) and remembers the rotated tokens.
    let again = engine.credentials(&discovered).await.unwrap();
    assert_eq!(again.client_id, "dyn-client-1");
    assert_eq!(state.lock().unwrap().registrations.len(), 1);
    assert_eq!(
        engine
            .stored_tokens(&discovered)
            .await
            .unwrap()
            .access_token,
        "access-2"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn iss_mismatch_rejects_before_token_exchange() {
    let state = Shared::default();
    state.lock().unwrap().iss_override = Some("https://attacker.example".into());
    let base = spawn_mock(Arc::clone(&state)).await;
    let engine = engine(&base);

    let discovered = engine.discover(None).await.unwrap();
    let credentials = engine.credentials(&discovered).await.unwrap();
    let pending = engine.begin(&discovered, &credentials, &[]).unwrap();
    let callback = browser(&pending.authorize_url).await;
    let err = engine
        .complete(&discovered, &credentials, pending, &callback)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            turbomcp_auth::client::OAuthClientError::IssuerMismatch { .. }
        ),
        "got {err}"
    );
    // The code never reached the token endpoint (RFC 9207 MUST).
    assert!(state.lock().unwrap().token_forms.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn state_mismatch_discards_the_response() {
    let state = Shared::default();
    let base = spawn_mock(Arc::clone(&state)).await;
    let engine = engine(&base);
    let discovered = engine.discover(None).await.unwrap();
    let credentials = engine.credentials(&discovered).await.unwrap();
    let pending = engine.begin(&discovered, &credentials, &[]).unwrap();
    let mut callback = browser(&pending.authorize_url).await;
    callback.state = Some("tampered".into());
    let err = engine
        .complete(&discovered, &credentials, pending, &callback)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("state mismatch"), "got {err}");
    assert!(state.lock().unwrap().token_forms.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_pkce_support_refuses_to_proceed() {
    let state = Shared::default();
    state.lock().unwrap().omit_pkce = true;
    let base = spawn_mock(Arc::clone(&state)).await;
    let err = engine(&base).discover(None).await.unwrap_err();
    assert!(
        matches!(
            err,
            turbomcp_auth::client::OAuthClientError::PkceUnsupported
        ),
        "got {err}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn metadata_issuer_impersonation_is_rejected() {
    let state = Shared::default();
    state.lock().unwrap().issuer_override = Some("https://honest.example".into());
    let base = spawn_mock(Arc::clone(&state)).await;
    let err = engine(&base).discover(None).await.unwrap_err();
    assert!(err.to_string().contains("issuer mismatch"), "got {err}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preregistered_credentials_bound_to_a_different_issuer_error_out() {
    let state = Shared::default();
    let base = spawn_mock(Arc::clone(&state)).await;
    let engine = OAuthClient::new(
        format!("{base}/mcp"),
        "http://127.0.0.1:19999/callback",
        RegistrationStrategy::Preregistered {
            credentials: turbomcp_auth::client::ClientCredentials::public("pre-1"),
            issuer: Some("https://old-as.example".into()),
        },
    );
    let discovered = engine.discover(None).await.unwrap();
    let err = engine.credentials(&discovered).await.unwrap_err();
    assert!(
        matches!(
            err,
            turbomcp_auth::client::OAuthClientError::IssuerChanged { .. }
        ),
        "got {err}"
    );
}

/// RFC 9207's other half: an AS that *advertises*
/// `authorization_response_iss_parameter_supported` and then answers without
/// `iss` must be refused. Accepting it would let an attacker strip the one
/// parameter that identifies which authorization server actually answered —
/// the mix-up defence is only worth anything if a missing `iss` fails closed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_advertised_iss_that_goes_missing_is_refused() {
    let state = Shared::default();
    state.lock().unwrap().omit_iss = true;
    let base = spawn_mock(Arc::clone(&state)).await;
    let engine = engine(&base);

    let discovered = engine.discover(None).await.unwrap();
    let credentials = engine.credentials(&discovered).await.unwrap();
    let pending = engine.begin(&discovered, &credentials, &[]).unwrap();
    let callback = browser(&pending.authorize_url).await;
    assert!(callback.iss.is_none(), "the mock omitted it");

    let err = engine
        .complete(&discovered, &credentials, pending, &callback)
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("omitted"),
        "the message should say what is missing: {err}"
    );
    // Nothing was redeemed (the check runs before the token endpoint).
    assert!(state.lock().unwrap().token_forms.is_empty());
}

/// The user refusing consent is an ordinary outcome, not a malformed one: the
/// `error` and its description must reach the caller so it can tell "you
/// declined" from "something broke".
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_denied_authorization_surfaces_its_error_and_description() {
    let state = Shared::default();
    state.lock().unwrap().deny = Some((
        "access_denied".into(),
        Some("the user declined consent".into()),
    ));
    let base = spawn_mock(Arc::clone(&state)).await;
    let engine = engine(&base);

    let discovered = engine.discover(None).await.unwrap();
    let credentials = engine.credentials(&discovered).await.unwrap();
    let pending = engine.begin(&discovered, &credentials, &[]).unwrap();
    let callback = browser(&pending.authorize_url).await;

    let err = engine
        .complete(&discovered, &credentials, pending, &callback)
        .await
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("access_denied"), "{msg}");
    assert!(msg.contains("the user declined consent"), "{msg}");
    assert!(state.lock().unwrap().token_forms.is_empty());
}

/// A redirect that passes every check and still carries no `code` — an AS bug,
/// or a truncated URL pasted by hand. It must be a named error rather than an
/// exchange attempt with an empty code.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_callback_with_neither_code_nor_error_is_rejected() {
    let state = Shared::default();
    let base = spawn_mock(Arc::clone(&state)).await;
    let engine = engine(&base);

    let discovered = engine.discover(None).await.unwrap();
    let credentials = engine.credentials(&discovered).await.unwrap();
    let pending = engine.begin(&discovered, &credentials, &[]).unwrap();
    let callback = CallbackParams::from_query(&format!(
        "?state={}&iss={}",
        urlencode(&pending.state),
        urlencode(&base)
    ));

    let err = engine
        .complete(&discovered, &credentials, pending, &callback)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("no code"), "got {err}");
    assert!(state.lock().unwrap().token_forms.is_empty());
}

/// Refresh-token rotation is a SHOULD, not a MUST. An authorization server
/// that answers a refresh without a new one has kept the old one live — and
/// dropping it here would make the *next* refresh fail and force the user back
/// through interactive consent for no reason.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_non_rotating_refresh_keeps_the_existing_token() {
    let state = Shared::default();
    state.lock().unwrap().no_refresh_rotation = true;
    let base = spawn_mock(Arc::clone(&state)).await;
    let engine = engine(&base);

    let discovered = engine.discover(None).await.unwrap();
    let credentials = engine.credentials(&discovered).await.unwrap();
    let pending = engine.begin(&discovered, &credentials, &[]).unwrap();
    let callback = browser(&pending.authorize_url).await;
    let tokens = engine
        .complete(&discovered, &credentials, pending, &callback)
        .await
        .unwrap();

    let rotated = engine
        .refresh(&discovered, &credentials, &tokens)
        .await
        .unwrap();
    assert_eq!(rotated.access_token, "access-2");
    assert_eq!(
        rotated.refresh_token.as_deref(),
        Some("refresh-1"),
        "the previous refresh token must survive a non-rotating response"
    );
    // And it is what gets persisted, so a later refresh still works.
    assert_eq!(
        engine
            .stored_tokens(&discovered)
            .await
            .unwrap()
            .refresh_token
            .as_deref(),
        Some("refresh-1")
    );
}

/// Refreshing a token set that never had a refresh token is a caller mistake
/// with an obvious remedy, so it must say so rather than fail somewhere inside
/// the token endpoint.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn refreshing_without_a_refresh_token_says_what_to_do() {
    let state = Shared::default();
    let base = spawn_mock(Arc::clone(&state)).await;
    let engine = engine(&base);

    let discovered = engine.discover(None).await.unwrap();
    let credentials = engine.credentials(&discovered).await.unwrap();
    let pending = engine.begin(&discovered, &credentials, &[]).unwrap();
    let callback = browser(&pending.authorize_url).await;
    let mut tokens = engine
        .complete(&discovered, &credentials, pending, &callback)
        .await
        .unwrap();
    // An AS that issues no refresh token at all (a public client on a
    // short-lived grant) leaves the set in exactly this shape.
    tokens.refresh_token = None;
    state.lock().unwrap().token_forms.clear();

    let err = engine
        .refresh(&discovered, &credentials, &tokens)
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("re-authorization required"),
        "got {err}"
    );
    assert!(
        state.lock().unwrap().token_forms.is_empty(),
        "nothing should reach the token endpoint"
    );
}
