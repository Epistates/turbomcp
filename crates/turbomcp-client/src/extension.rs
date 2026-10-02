//! The client half of an extension; see [`ClientExtension`].

use async_trait::async_trait;
use serde_json::{Map, Value};

use crate::{Client, ClientError, ClientResult};

/// The client half of an extension (SEP-2133): what a client declares, the
/// `resultType`s it understands, and the notifications it handles.
///
/// [`ClientBuilder::with_extension`](crate::ClientBuilder::with_extension)
/// only declares an extension. A [`ClientExtension`] also teaches the client
/// what the extension sends back: a `2026-07-28` result whose `resultType`
/// the extension claims is handed to [`settle`](ClientExtension::settle) to
/// become the final result (where an unclaimed one is refused, as the spec
/// requires), and notifications it names reach
/// [`on_notification`](ClientExtension::on_notification) rather than the
/// general [`NotificationHandler`](crate::NotificationHandler).
///
/// Register with
/// [`ClientBuilder::with_client_extension`](crate::ClientBuilder::with_client_extension).
/// Collisions are refused when the client is built: two extensions with one
/// id, claiming one `resultType` or one notification, or claiming the core
/// `complete`/`input_required` result types.
///
/// The built-in Tasks support (`io.modelcontextprotocol/tasks`) stays native:
/// its detached and resumable task handles are API of their own, which a
/// settle-to-a-result hook can't express.
#[async_trait]
pub trait ClientExtension: Send + Sync + 'static {
    /// The extension's id (`com.example/thing`), declared in the client's
    /// capabilities.
    fn id(&self) -> &str;

    /// The settings declared with it (default `{}`).
    fn settings(&self) -> Value {
        Value::Object(Map::new())
    }

    /// The `resultType`s this extension answers for.
    fn result_types(&self) -> &[&str] {
        &[]
    }

    /// Turn `result`, an answer to `method` carrying one of
    /// [`result_types`](Self::result_types), into the final result the
    /// request resolves to (driving whatever the extension defines to get
    /// there). The default refuses it.
    ///
    /// # Errors
    /// Whatever settling fails with; the request fails with it.
    async fn settle(&self, client: &Client, method: &str, result: Value) -> ClientResult<Value> {
        let _ = (client, result);
        Err(ClientError::Protocol(format!(
            "extension `{}` claims a resultType on `{method}` but does not settle it",
            self.id()
        )))
    }

    /// The notification methods this extension handles.
    fn notifications(&self) -> &[&str] {
        &[]
    }

    /// One of [`notifications`](Self::notifications) arrived.
    async fn on_notification(&self, method: &str, params: Option<Value>) {
        let _ = (method, params);
    }
}

/// Refuse a registration that would collide with one already made: the
/// first match winning silently is how plugins corrupt each other.
///
/// # Panics
/// On a duplicate id, a `resultType` or notification claimed twice, or a
/// claim on a core result type.
pub(crate) fn check_registration(
    existing: &[std::sync::Arc<dyn ClientExtension>],
    new: &dyn ClientExtension,
) {
    use turbomcp_protocol::neutral::result_type::{COMPLETE, INPUT_REQUIRED};
    for claimed in new.result_types() {
        assert!(
            *claimed != COMPLETE && *claimed != INPUT_REQUIRED,
            "client extension `{}` claims the core resultType `{claimed}`",
            new.id()
        );
    }
    for other in existing {
        assert!(
            other.id() != new.id(),
            "client extension `{}` registered twice",
            new.id()
        );
        for claimed in new.result_types() {
            assert!(
                !other.result_types().contains(claimed),
                "client extensions `{}` and `{}` both claim resultType `{claimed}`",
                other.id(),
                new.id()
            );
        }
        for method in new.notifications() {
            assert!(
                !other.notifications().contains(method),
                "client extensions `{}` and `{}` both claim `{method}`",
                other.id(),
                new.id()
            );
        }
    }
}
