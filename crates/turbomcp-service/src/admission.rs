//! A request's place under a transport's in-flight limit, and who holds it.

use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::OwnedSemaphorePermit;

/// The slot a request was admitted on: its place under a transport's
/// in-flight limit (the Streamable HTTP endpoint's `with_max_in_flight`).
///
/// The transport puts one on the request and gives the slot back when its
/// own part ends ([`release`](Self::release)). Whatever goes on running the
/// request's work past that point [takes](Self::take) the slot and holds it
/// until the work ends: a stream with a slot of its own, or a handler that
/// runs on after its request was abandoned. A slot taken leaves every clone
/// empty, so a clone something else keeps (a request context a handler
/// stored) never holds a place past the work.
#[derive(Clone, Debug)]
pub struct Admission(Arc<Mutex<Option<OwnedSemaphorePermit>>>);

impl Admission {
    /// The slot `permit` holds.
    #[must_use]
    pub fn new(permit: OwnedSemaphorePermit) -> Self {
        Self(Arc::new(Mutex::new(Some(permit))))
    }

    /// Give the slot back, unless it has been taken.
    pub fn release(&self) {
        self.0.lock().take();
    }

    /// Take the slot, to hold until the work it admitted ends. `None` once
    /// taken or released.
    #[must_use]
    pub fn take(&self) -> Option<OwnedSemaphorePermit> {
        self.0.lock().take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::Semaphore;

    #[test]
    fn a_taken_slot_outlives_its_release() {
        let slots = Arc::new(Semaphore::new(1));
        let admission = Admission::new(Arc::clone(&slots).try_acquire_owned().unwrap());
        let kept = admission.clone();
        let held = admission.take();
        admission.release();
        assert_eq!(slots.available_permits(), 0, "the taker still holds it");
        assert!(kept.take().is_none(), "every clone is empty once taken");
        drop(held);
        assert_eq!(slots.available_permits(), 1);
    }

    #[test]
    fn release_gives_back_a_slot_nobody_took() {
        let slots = Arc::new(Semaphore::new(1));
        let admission = Admission::new(Arc::clone(&slots).try_acquire_owned().unwrap());
        let _kept = admission.clone();
        admission.release();
        assert_eq!(
            slots.available_permits(),
            1,
            "a clone kept elsewhere holds nothing"
        );
    }
}
