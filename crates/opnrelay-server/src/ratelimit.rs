// SPDX-License-Identifier: AGPL-3.0-only
//! Small in-memory fixed-window rate limiter (single-node deployment).

use std::time::{Duration, Instant};

use dashmap::DashMap;

#[derive(Default)]
pub struct RateLimiter {
    buckets: DashMap<String, (u32, Instant)>,
}

impl RateLimiter {
    /// Record a hit for `key`; returns `false` when the limit is exceeded.
    pub fn check(&self, key: &str, limit: u32, window: Duration) -> bool {
        let now = Instant::now();
        if self.buckets.len() > 50_000 {
            self.buckets
                .retain(|_, (_, start)| now.duration_since(*start) < Duration::from_secs(3600));
        }
        let mut entry = self.buckets.entry(key.to_string()).or_insert((0, now));
        let (count, start) = entry.value_mut();
        if now.duration_since(*start) >= window {
            *count = 0;
            *start = now;
        }
        *count += 1;
        *count <= limit
    }

    pub fn reset(&self, key: &str) {
        self.buckets.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_and_resets() {
        let r = RateLimiter::default();
        let w = Duration::from_secs(60);
        assert!(r.check("a", 2, w));
        assert!(r.check("a", 2, w));
        assert!(!r.check("a", 2, w));
        assert!(r.check("b", 2, w));
        r.reset("a");
        assert!(r.check("a", 2, w));
        assert!(r.check("z", 1, Duration::ZERO));
        assert!(r.check("z", 1, Duration::ZERO));
    }
}
