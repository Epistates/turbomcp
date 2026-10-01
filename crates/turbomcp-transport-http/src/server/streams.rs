//! What the endpoint's long-lived streams may hold: their own budget, apart
//! from the requests.
//!
//! A legacy `GET` stream or a `subscriptions/listen` stream stays open for as
//! long as the client keeps it, and a keep-alive every few seconds costs the
//! client nothing. When those streams counted against the request admission
//! pool, one client could open enough idle ones to refuse every request from
//! everyone. They now draw on a separate pool, capped per client too, and
//! give their request slot back the moment they become a stream.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::response::Response;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use turbomcp_service::RateKey;

use super::reject::{service_unavailable, too_many_streams};

/// The request slot a request was admitted on. The admission middleware puts
/// one on every request and holds it until the response body ends; a
/// handler whose response is a long-lived stream [releases](Self::release)
/// it once the stream has its own [`StreamSlot`].
#[derive(Clone)]
pub(super) struct Admission(Arc<Mutex<Option<OwnedSemaphorePermit>>>);

impl Admission {
    pub(super) fn new(permit: OwnedSemaphorePermit) -> Self {
        Self(Arc::new(Mutex::new(Some(permit))))
    }

    /// Give the request slot back.
    pub(super) fn release(&self) {
        self.0.lock().expect("admission slot poisoned").take();
    }

    /// Take the request slot to hold elsewhere: a call that runs on a task of
    /// its own keeps it until the call ends, not until the response does.
    pub(super) fn take(&self) -> Option<OwnedSemaphorePermit> {
        self.0.lock().expect("admission slot poisoned").take()
    }
}

/// How many streams each caller has open.
type OpenCounts = Arc<Mutex<HashMap<RateKey, usize>>>;

/// Open long-lived streams: at most `max` in all and `per_client` for any one
/// caller.
#[derive(Clone)]
pub(super) struct StreamBudget {
    slots: Arc<Semaphore>,
    per_client: usize,
    open: OpenCounts,
}

impl StreamBudget {
    pub(super) fn new(max: usize, per_client: usize) -> Self {
        Self {
            slots: Arc::new(Semaphore::new(max)),
            per_client,
            open: Arc::default(),
        }
    }

    /// A slot for one more stream from `client`, or the response refusing it:
    /// `503` when the endpoint as a whole is full, `429` when this caller is.
    ///
    /// A caller the endpoint cannot tell apart from others ([`RateKey::Global`]:
    /// no identity and no peer address, as under a router served without
    /// connect info) is held only to the endpoint-wide limit, since a
    /// per-client cap would be shared by everyone.
    pub(super) fn admit(&self, client: RateKey) -> Result<StreamSlot, Box<Response>> {
        let Ok(permit) = Arc::clone(&self.slots).try_acquire_owned() else {
            return Err(Box::new(service_unavailable(
                "too many open streams on this endpoint",
            )));
        };
        let counted = if client == RateKey::Global {
            None
        } else {
            let mut open = self.open.lock().expect("stream counts poisoned");
            let count = open.entry(client.clone()).or_default();
            if *count >= self.per_client {
                return Err(Box::new(too_many_streams()));
            }
            *count += 1;
            Some((Arc::clone(&self.open), client))
        };
        Ok(StreamSlot {
            _permit: permit,
            counted,
        })
    }
}

/// One open stream's share of the [`StreamBudget`], returned when the stream
/// ends (it travels inside the stream's state).
pub(super) struct StreamSlot {
    _permit: OwnedSemaphorePermit,
    counted: Option<(OpenCounts, RateKey)>,
}

impl Drop for StreamSlot {
    fn drop(&mut self) {
        let Some((open, client)) = self.counted.take() else {
            return;
        };
        let mut open = open.lock().expect("stream counts poisoned");
        if let Some(count) = open.get_mut(&client) {
            *count -= 1;
            if *count == 0 {
                open.remove(&client);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    fn ip(last: u8) -> RateKey {
        RateKey::Ip(IpAddr::V4(Ipv4Addr::new(192, 0, 2, last)))
    }

    #[test]
    fn a_client_is_held_to_its_share_and_gets_it_back() {
        let budget = StreamBudget::new(8, 2);
        let a = budget.admit(ip(1)).unwrap();
        let _b = budget.admit(ip(1)).unwrap();
        assert!(budget.admit(ip(1)).is_err());
        assert!(
            budget.admit(ip(2)).is_ok(),
            "another client has its own share"
        );
        drop(a);
        assert!(budget.admit(ip(1)).is_ok());
    }

    #[test]
    fn the_endpoint_limit_holds_across_clients() {
        let budget = StreamBudget::new(2, 8);
        let _a = budget.admit(ip(1)).unwrap();
        let _b = budget.admit(ip(2)).unwrap();
        assert!(budget.admit(ip(3)).is_err());
    }

    #[test]
    fn anonymous_callers_share_only_the_endpoint_limit() {
        let budget = StreamBudget::new(4, 1);
        let _a = budget.admit(RateKey::Global).unwrap();
        let _b = budget.admit(RateKey::Global).unwrap();
        assert!(budget.open.lock().unwrap().is_empty());
    }
}
