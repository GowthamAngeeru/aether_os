use dashmap::DashMap;
use parking_lot::Mutex;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug)]
struct BucketState {
    tokens: f64,
    last_updated: Instant,
}

#[derive(Debug)]
struct TokenBucket {
    capacity: f64,
    fill_rate: f64,
    state: Mutex<BucketState>,
}

impl TokenBucket {
    fn new(capacity: f64, fill_rate: f64) -> Self {
        Self {
            capacity,
            fill_rate,
            state: Mutex::new(BucketState {
                tokens: capacity,
                last_updated: Instant::now(),
            }),
        }
    }

    fn take(&self) -> bool {
        let mut state = self.state.lock();

        let now = Instant::now();
        let elapsed = now.duration_since(state.last_updated).as_secs_f64();

        state.tokens = (state.tokens + elapsed * self.fill_rate).min(self.capacity);
        state.last_updated = now;

        if state.tokens >= 1.0 {
            state.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[derive(Debug, Clone)]
pub struct RateLimiter {
    buckets: Arc<DashMap<IpAddr, TokenBucket>>,
    capacity: f64,
    fill_rate: f64,
}

impl RateLimiter {
    pub fn new(capacity: f64, fill_rate: f64) -> Self {
        Self {
            buckets: Arc::new(DashMap::new()),
            capacity,
            fill_rate,
        }
    }

    pub fn is_allowed(&self, ip: IpAddr) -> bool {
        let bucket = self
            .buckets
            .entry(ip)
            .or_insert_with(|| TokenBucket::new(self.capacity, self.fill_rate));

        bucket.take()
    }
}
