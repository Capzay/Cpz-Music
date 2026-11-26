use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::http::HeaderMap;

/// Fixed-window rate limiter. In-process, so counters reset on restart; this
/// server runs as a single process by design.
#[derive(Default)]
pub struct RateLimiter {
    windows: Mutex<HashMap<String, (u32, Instant)>>,
}

impl RateLimiter {
    pub fn check(&self, key: &str, limit: u32, window: Duration) -> bool {
        let now = Instant::now();
        let mut windows = self.windows.lock().unwrap();

        match windows.get_mut(key) {
            Some((count, reset_at)) if now < *reset_at => {
                if *count >= limit {
                    return false;
                }
                *count += 1;
                true
            }
            _ => {
                windows.insert(key.to_owned(), (1, now + window));
                // Opportunistic sweep so an attacker cannot grow this map without
                // bound by varying the key.
                if windows.len() > 10_000 {
                    windows.retain(|_, (_, reset_at)| now < *reset_at);
                }
                true
            }
        }
    }
}

/// Best-effort client address for rate-limit keys behind Cloudflare Tunnel.
pub fn client_key(headers: &HeaderMap) -> String {
    let header = |name: &str| headers.get(name).and_then(|value| value.to_str().ok());
    header("cf-connecting-ip")
        .or_else(|| {
            header("x-forwarded-for")
                .and_then(|v| v.split(',').next())
                .map(str::trim)
        })
        .unwrap_or("unknown")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_up_to_the_limit_then_refuses() {
        let limiter = RateLimiter::default();
        let minute = Duration::from_secs(60);
        assert!(limiter.check("a", 2, minute));
        assert!(limiter.check("a", 2, minute));
        assert!(!limiter.check("a", 2, minute));
        assert!(limiter.check("b", 2, minute));
    }

    #[test]
    fn a_new_window_starts_after_the_old_one_ends() {
        let limiter = RateLimiter::default();
        assert!(limiter.check("a", 1, Duration::ZERO));
        assert!(limiter.check("a", 1, Duration::ZERO));
    }

    #[test]
    fn prefers_the_cloudflare_address() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "1.1.1.1, 2.2.2.2".parse().unwrap());
        assert_eq!(client_key(&headers), "1.1.1.1");
        headers.insert("cf-connecting-ip", "3.3.3.3".parse().unwrap());
        assert_eq!(client_key(&headers), "3.3.3.3");
    }
}
