use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::sync::Mutex;

const LOGIN_WINDOW: Duration = Duration::from_secs(10 * 60);
const MAX_FAILURES: usize = 5;

#[derive(Clone, Default)]
pub struct LoginLimiter {
    attempts: Arc<Mutex<HashMap<String, VecDeque<Instant>>>>,
}

impl LoginLimiter {
    pub async fn is_allowed(&self, key: &str) -> bool {
        let mut attempts = self.attempts.lock().await;
        let history = attempts.entry(key.to_owned()).or_default();
        remove_expired(history);
        history.len() < MAX_FAILURES
    }

    pub async fn record_failure(&self, key: &str) {
        let mut attempts = self.attempts.lock().await;
        let history = attempts.entry(key.to_owned()).or_default();
        remove_expired(history);
        history.push_back(Instant::now());
    }

    pub async fn clear(&self, key: &str) {
        self.attempts.lock().await.remove(key);
    }
}

fn remove_expired(history: &mut VecDeque<Instant>) {
    let cutoff = Instant::now() - LOGIN_WINDOW;
    while history.front().is_some_and(|attempt| *attempt < cutoff) {
        history.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn blocks_after_five_failures_and_clears_on_success() {
        let limiter = LoginLimiter::default();
        for _ in 0..MAX_FAILURES {
            assert!(limiter.is_allowed("127.0.0.1:alice").await);
            limiter.record_failure("127.0.0.1:alice").await;
        }
        assert!(!limiter.is_allowed("127.0.0.1:alice").await);
        limiter.clear("127.0.0.1:alice").await;
        assert!(limiter.is_allowed("127.0.0.1:alice").await);
    }
}
