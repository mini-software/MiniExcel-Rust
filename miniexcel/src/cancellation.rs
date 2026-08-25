use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use event_listener::Event;

#[derive(Debug, Default)]
struct CancellationState {
    cancelled: AtomicBool,
    event: Event,
}

/// A runtime-neutral cooperative cancellation signal for async operations.
#[derive(Clone, Debug, Default)]
pub struct CancellationToken {
    state: Arc<CancellationState>,
}

impl CancellationToken {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        if !self.state.cancelled.swap(true, Ordering::AcqRel) {
            self.state.event.notify(usize::MAX);
        }
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.state.cancelled.load(Ordering::Acquire)
    }

    pub(crate) async fn cancelled(&self) {
        loop {
            if self.is_cancelled() {
                return;
            }
            let listener = self.state.event.listen();
            if self.is_cancelled() {
                return;
            }
            listener.await;
        }
    }
}
