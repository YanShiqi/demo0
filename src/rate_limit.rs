use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::sync::Mutex;

const LOGIN_WINDOW: Duration = Duration::from_secs(10 * 60);
const MAX_FAILURES: usize = 5;

/// 用于敏感查询的可配置滑动窗口尝试限制器。
#[derive(Clone)]
pub struct AttemptLimiter {
    attempts: Arc<Mutex<HashMap<String, VecDeque<Instant>>>>,
    window: Duration,
    max_attempts: usize,
}

impl AttemptLimiter {
    pub fn new(window: Duration, max_attempts: usize) -> Self {
        Self {
            attempts: Arc::new(Mutex::new(HashMap::new())),
            window,
            max_attempts,
        }
    }

    /// 记录本次尝试；达到窗口上限时拒绝且不增加无效记录。
    pub async fn check_and_record(&self, key: &str) -> bool {
        let mut attempts = self.attempts.lock().await;
        let history = attempts.entry(key.to_owned()).or_default();
        remove_expired_with_window(history, self.window);
        if history.len() >= self.max_attempts {
            return false;
        }
        history.push_back(Instant::now());
        true
    }
}

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
    remove_expired_with_window(history, LOGIN_WINDOW);
}

fn remove_expired_with_window(history: &mut VecDeque<Instant>, window: Duration) {
    let cutoff = Instant::now() - window;
    while history.front().is_some_and(|attempt| *attempt < cutoff) {
        history.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn configurable_attempt_limiter_blocks_after_the_limit() {
        let limiter = AttemptLimiter::new(Duration::from_secs(60), 2);

        assert!(limiter.check_and_record("actor-1").await);
        assert!(limiter.check_and_record("actor-1").await);
        assert!(!limiter.check_and_record("actor-1").await);
    }

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
