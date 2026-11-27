//! Listening history. A listen that cannot reach the server is kept and replayed
//! later with its original timestamp, so it lands on the day it happened.

use std::sync::Arc;

use tokio::sync::Mutex;

use crate::api::Api;
use crate::http::now_ms;
use crate::store::Store;

/// Below this, a play was a skip rather than a listen.
pub const MIN_LISTEN_SECS: f64 = 30.0;

pub fn counts_as_listen(seconds: f64, completed: bool) -> bool {
    completed || seconds >= MIN_LISTEN_SECS
}

pub struct Listens {
    store: Arc<Store>,
    /// One replay at a time, or two could send the same queued listen twice.
    flushing: Mutex<()>,
}

impl Listens {
    pub fn new(store: Arc<Store>) -> Self {
        Self {
            store,
            flushing: Mutex::new(()),
        }
    }

    pub async fn report(&self, api: &Api, track_id: i32, seconds: f64, completed: bool) {
        if !counts_as_listen(seconds, completed) {
            return;
        }
        let secs = seconds.round() as i64;
        let at = now_ms();
        match api.record_listen(track_id, secs, at).await {
            Ok(()) => {}
            Err(err) if err.is_retryable() => {
                if let Err(err) = self.store.queue_listen(track_id, secs, at) {
                    tracing::warn!("could not queue a listen: {err}");
                }
            }
            // The server will never take this one, for instance a deleted track.
            Err(err) => tracing::debug!("dropped a listen: {err}"),
        }
    }

    /// Replays queued listens oldest first, stopping at the first that still
    /// cannot get through.
    pub async fn flush(&self, api: &Api) {
        let Ok(_guard) = self.flushing.try_lock() else {
            return;
        };
        let Ok(queued) = self.store.queued_listens() else {
            return;
        };
        for listen in queued {
            match api
                .record_listen(listen.track_id, listen.duration_secs, listen.at)
                .await
            {
                Err(err) if err.is_retryable() => return,
                _ => {
                    let _ = self.store.delete_listen(listen.id);
                }
            }
        }
    }

    pub fn pending(&self) -> usize {
        self.store.queued_listens().map_or(0, |queued| queued.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_do_not_count_but_short_finished_tracks_do() {
        assert!(!counts_as_listen(29.0, false));
        assert!(counts_as_listen(30.0, false));
        assert!(counts_as_listen(12.0, true));
    }
}
