//! Public OAuth coordinator concurrency, stale challenges, and refresh rotation.
#![cfg(feature = "client-oauth")]
use axum::{
    Json, Router,
    extract::Form,
    routing::{get, post},
};
use serde_json::json;
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use turbomcp::{
    auth::client::{CallbackParams, ClientCredentials, OAuthClient, RegistrationStrategy},
    client::{
        BearerSource,
        oauth::{AuthorizationHandler, OAuthSession},
    },
};

struct Consent {
    calls: AtomicUsize,
}
#[turbomcp::client::async_trait]
impl AuthorizationHandler for Consent {
    async fn authorize(&self, url: &str) -> Result<CallbackParams, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let url = reqwest::Url::parse(url).unwrap();
        let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(query["code_challenge_method"], "S256");
        assert!(!query["code_challenge"].is_empty());
        tokio::task::yield_now().await;
        Ok(CallbackParams::from_query(&format!(
            "code=granted&state={}",
            query["state"]
        )))
    }
}
struct Fixture {
    session: Arc<OAuthSession>,
    consent: Arc<Consent>,
    grants: Arc<AtomicUsize>,
    refreshes: Arc<AtomicUsize>,
    server: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}
async fn fixture(expiring: bool, reject_refresh: bool) -> Fixture {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let resource = format!("{base}/mcp");
    let metadata =
        json!({"resource":resource,"authorization_servers":[base],"scopes_supported":["tools"]});
    let issuer = json!({"issuer":base,"authorization_endpoint":format!("{base}/authorize"),"token_endpoint":format!("{base}/token"),"code_challenge_methods_supported":["S256"]});
    let grants = Arc::new(AtomicUsize::new(0));
    let refreshes = Arc::new(AtomicUsize::new(0));
    let g = grants.clone();
    let r = refreshes.clone();
    let expected_resource = resource.clone();
    let app=Router::new()
        .route("/.well-known/oauth-protected-resource/mcp", get(move || {let v=metadata.clone(); async move {Json(v)}}))
        .route("/.well-known/oauth-authorization-server", get(move || {let v=issuer.clone(); async move {Json(v)}}))
        .route("/token", post(move |Form(form): Form<HashMap<String,String>>| {
            let g=g.clone(); let r=r.clone(); let resource=expected_resource.clone(); async move {
                assert_eq!(form["resource"], resource);
                let refresh=form["grant_type"]=="refresh_token";
                if refresh { assert_eq!(form["refresh_token"], "refresh-one"); r.fetch_add(1,Ordering::SeqCst); }
                else { assert!(!form["code_verifier"].is_empty()); g.fetch_add(1,Ordering::SeqCst); }
                tokio::task::yield_now().await;
                if refresh && reject_refresh {
                    return (axum::http::StatusCode::BAD_REQUEST, Json(json!({"error":"invalid_grant"})));
                }
                (axum::http::StatusCode::OK, Json(json!({"access_token":if refresh {"access-two"} else {"access-one"},"refresh_token":if refresh {"refresh-two"} else {"refresh-one"},"token_type":"Bearer","expires_in":if expiring && !refresh {1} else {3600},"scope":"tools"})))
            }
        }));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let consent = Arc::new(Consent {
        calls: AtomicUsize::new(0),
    });
    let session = Arc::new(OAuthSession::new(
        OAuthClient::new(
            resource,
            "http://localhost/callback",
            RegistrationStrategy::Preregistered {
                credentials: ClientCredentials::public("test-client"),
                issuer: Some(base),
            },
        ),
        consent.clone(),
    ));
    Fixture {
        session,
        consent,
        grants,
        refreshes,
        server,
    }
}

#[tokio::test]
async fn simultaneous_challenges_authorize_once_and_stale_rejections_reuse_token() {
    let f = fixture(false, false).await;
    let barrier = Arc::new(tokio::sync::Barrier::new(24));
    let mut jobs = tokio::task::JoinSet::new();
    for _ in 0..24 {
        let s = f.session.clone();
        let b = barrier.clone();
        jobs.spawn(async move {
            b.wait().await;
            assert!(s.on_challenge(401, None, None).await.unwrap());
        });
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(j) = jobs.join_next().await {
            j.unwrap();
        }
    })
    .await
    .unwrap();
    assert_eq!(f.consent.calls.load(Ordering::SeqCst), 1);
    assert_eq!(f.grants.load(Ordering::SeqCst), 1);
    assert_eq!(f.session.bearer().await.as_deref(), Some("access-one"));
    assert!(
        f.session
            .on_challenge(401, None, Some("obsolete"))
            .await
            .unwrap()
    );
    assert!(
        !f.session
            .on_challenge(403, None, Some("access-one"))
            .await
            .unwrap()
    );
    assert!(
        !f.session
            .on_challenge(500, None, Some("access-one"))
            .await
            .unwrap()
    );
    assert_eq!(f.consent.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn simultaneous_expiring_token_reads_perform_one_refresh() {
    let f = fixture(true, false).await;
    assert!(f.session.on_challenge(401, None, None).await.unwrap());
    let barrier = Arc::new(tokio::sync::Barrier::new(24));
    let mut jobs = tokio::task::JoinSet::new();
    for _ in 0..24 {
        let s = f.session.clone();
        let b = barrier.clone();
        jobs.spawn(async move {
            b.wait().await;
            assert_eq!(s.bearer().await.as_deref(), Some("access-two"));
        });
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(j) = jobs.join_next().await {
            j.unwrap();
        }
    })
    .await
    .unwrap();
    assert_eq!(f.refreshes.load(Ordering::SeqCst), 1);
    assert_eq!(f.grants.load(Ordering::SeqCst), 1);
    assert_eq!(f.consent.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn rejected_refresh_is_not_retried_by_every_waiting_caller() {
    let f = fixture(true, true).await;
    assert!(f.session.on_challenge(401, None, None).await.unwrap());
    let barrier = Arc::new(tokio::sync::Barrier::new(24));
    let mut jobs = tokio::task::JoinSet::new();
    for _ in 0..24 {
        let s = f.session.clone();
        let b = barrier.clone();
        jobs.spawn(async move {
            b.wait().await;
            assert_eq!(s.bearer().await, None);
        });
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(j) = jobs.join_next().await {
            j.unwrap();
        }
    })
    .await
    .unwrap();
    assert_eq!(f.refreshes.load(Ordering::SeqCst), 1);
    assert_eq!(f.consent.calls.load(Ordering::SeqCst), 1);
    // A new protected-resource challenge can start a fresh consent flow.
    assert!(f.session.on_challenge(401, None, None).await.unwrap());
    assert_eq!(f.consent.calls.load(Ordering::SeqCst), 2);
}
