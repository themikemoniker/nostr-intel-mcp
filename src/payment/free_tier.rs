use std::sync::Arc;

use crate::nostr::cache::Cache;

pub struct FreeTierLimiter {
    cache: Arc<Cache>,
}

impl FreeTierLimiter {
    pub fn new(cache: Arc<Cache>) -> Self {
        Self { cache }
    }

    /// Returns true if the client is under the rate limit (and increments the counter).
    /// Returns false if the limit has been exhausted.
    /// Fails open: if SQLite errors, allows the call.
    pub async fn check_and_increment(&self, client_id: &str, limit: u32) -> bool {
        let today = current_day();
        match self.cache.check_and_increment_rate(client_id, today, limit).await {
            Ok(allowed) => allowed,
            Err(e) => {
                tracing::warn!("Rate limit check failed (allowing call): {e}");
                true // fail-open
            }
        }
    }

    /// Get the current count of calls used today for a client.
    /// Returns 0 on error.
    pub async fn get_current_count(&self, client_id: &str) -> u32 {
        let today = current_day();
        match self.cache.get_rate_count(client_id, today).await {
            Ok(count) => count,
            Err(e) => {
                tracing::warn!("Rate limit count query failed: {e}");
                0
            }
        }
    }
}

fn current_day() -> u32 {
    use chrono::Datelike;
    chrono::Utc::now().ordinal()
}
