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
        match self
            .cache
            .check_and_increment_rate(client_id, today, limit)
            .await
        {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nostr::cache::Cache;

    #[tokio::test]
    async fn limiter_check_and_count() {
        let cache = Arc::new(Cache::new_in_memory().await);
        let limiter = FreeTierLimiter::new(cache);

        // First call should be allowed
        assert!(limiter.check_and_increment("test-session", 5).await);
        assert_eq!(limiter.get_current_count("test-session").await, 1);

        // Use up remaining calls
        for _ in 0..4 {
            assert!(limiter.check_and_increment("test-session", 5).await);
        }
        assert_eq!(limiter.get_current_count("test-session").await, 5);

        // Next call should be blocked
        assert!(!limiter.check_and_increment("test-session", 5).await);
        // Count stays at 5 (not incremented past limit)
        assert_eq!(limiter.get_current_count("test-session").await, 5);
    }

    #[tokio::test]
    async fn limiter_independent_sessions() {
        let cache = Arc::new(Cache::new_in_memory().await);
        let limiter = FreeTierLimiter::new(cache);

        // Exhaust session A
        for _ in 0..3 {
            limiter.check_and_increment("session-a", 3).await;
        }
        assert!(!limiter.check_and_increment("session-a", 3).await);

        // Session B should still work
        assert!(limiter.check_and_increment("session-b", 3).await);
        assert_eq!(limiter.get_current_count("session-b").await, 1);
    }
}
