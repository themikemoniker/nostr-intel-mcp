use std::collections::HashMap;
use tokio::sync::RwLock;

struct DayCounter {
    count: u32,
    day: u32,
}

pub struct FreeTierLimiter {
    counters: RwLock<HashMap<String, DayCounter>>,
}

impl FreeTierLimiter {
    pub fn new() -> Self {
        Self {
            counters: RwLock::new(HashMap::new()),
        }
    }

    /// Returns true if the client is under the rate limit (and increments the counter).
    /// Returns false if the limit has been exhausted.
    pub async fn check_and_increment(&self, client_id: &str, limit: u32) -> bool {
        let today = current_day();
        let mut counters = self.counters.write().await;

        let counter = counters
            .entry(client_id.to_string())
            .or_insert(DayCounter { count: 0, day: today });

        // Reset if day changed
        if counter.day != today {
            counter.count = 0;
            counter.day = today;
        }

        if counter.count < limit {
            counter.count += 1;
            true
        } else {
            false
        }
    }
}

fn current_day() -> u32 {
    use chrono::Datelike;
    chrono::Utc::now().ordinal()
}
