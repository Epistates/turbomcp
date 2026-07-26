//! The OTLP installer (feature `otlp`).
//!
//! `init_otlp` registers a global tracer provider, a global meter provider, a
//! global propagator, and a global `tracing` subscriber — process-wide state
//! that can only be installed once. That makes it awkward to test and easy to
//! leave untested, which is how a turnkey installer ends up shipping broken:
//! nothing else in the suite calls it, so a bad exporter build or a mis-ordered
//! global registration would surface first in a user's production startup.
//!
//! An integration test gets its own process, so the one-shot install is safe
//! here. No collector is required — the tonic exporters connect lazily, so a
//! successful build proves the pipeline is assembled without needing one
//! listening on 4317.

#![cfg(feature = "otlp")]

use turbomcp_telemetry::{OtlpConfig, TelemetryError, init_otlp};

#[test]
fn config_defaults_to_the_collectors_own_default_endpoint() {
    let config = OtlpConfig::new("svc");
    assert_eq!(config.service_name, "svc");
    assert_eq!(
        config.endpoint, None,
        "an unset endpoint must stay unset so the exporter picks its own \
         default, rather than this crate hard-coding one"
    );
    assert_eq!(
        OtlpConfig::new("svc").endpoint("http://otel:4317").endpoint,
        Some("http://otel:4317".to_string())
    );
}

/// Installs the pipeline, then proves the second attempt is a clean error.
///
/// Both halves must live in one test: the install is global, so a second test
/// in this process would race it and the failure mode would depend on test
/// ordering. `try_init` on an already-installed subscriber is the realistic
/// misuse — an embedder that also sets up `tracing` — and it must surface as
/// `TelemetryError::Subscriber`, not a panic.
#[tokio::test]
async fn init_installs_once_and_reports_a_second_attempt() {
    let guard = init_otlp(OtlpConfig::new("turbomcp-test").endpoint("http://127.0.0.1:4317"))
        .expect("the OTLP pipeline builds without a live collector");

    match init_otlp(OtlpConfig::new("turbomcp-test-again")) {
        Err(TelemetryError::Subscriber(msg)) => {
            assert!(!msg.is_empty(), "the error should say what went wrong");
        }
        Err(other) => panic!("expected a Subscriber error, got {other:?}"),
        Ok(_) => panic!("a second global subscriber must not install"),
    }

    // Dropping the guard shuts both providers down; it must not panic even
    // though nothing is listening on the endpoint.
    drop(guard);
}
