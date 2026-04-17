// src/core/cache.rs

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use dashmap::DashMap;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::core::vector::{VectorEngine, SIMILARITY_THRESHOLD};

// ─── Constants ────────────────────────────────────────────────────────────────

const MAX_CACHE_CAPACITY: usize = 50_000;
const EVICTION_BATCH_SIZE: usize = 500;
pub const DEFAULT_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const CAPACITY_WARN_THRESHOLD: f64 = 0.80;

const REDIS_KEY_PREFIX: &str = "aetheros:cache:";
const REDIS_SCAN_PATTERN: &str = "aetheros:cache:*";

// ─── Cache Entry ─────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct CachedEntry {
    pub vector: Vec<f32>,
    pub response: String,
    pub inserted_at: Instant,
    pub hit_count: u64,
}

impl CachedEntry {
    fn new(vector: Vec<f32>, response: String, age_secs: u64) -> Self {
        let inserted_at = Instant::now()
            .checked_sub(Duration::from_secs(age_secs))
            .unwrap_or_else(Instant::now);

        Self {
            vector,
            response,
            inserted_at,
            hit_count: 0,
        }
    }

    fn is_expired(&self, ttl: Duration) -> bool {
        self.inserted_at.elapsed() > ttl
    }
}

// ─── Redis Serialization Entry ───────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct RedisEntry {
    vector: Vec<f32>,
    response: String,
    inserted_at_unix: u64,
}

impl RedisEntry {
    fn new(vector: Vec<f32>, response: String) -> Self {
        let inserted_at_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            vector,
            response,
            inserted_at_unix,
        }
    }

    fn age_secs(&self) -> u64 {
        let now_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now_unix.saturating_sub(self.inserted_at_unix)
    }
}

fn redis_key(prompt: &str) -> String {
    format!("{}{}", REDIS_KEY_PREFIX, prompt)
}

// ─── Cache Metrics ────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct CacheMetrics {
    pub hits: AtomicU64,
    pub misses: AtomicU64,
    pub insertions: AtomicU64,
    pub evictions: AtomicU64,
    pub expired_hits: AtomicU64,
}

impl CacheMetrics {
    pub fn hit_rate(&self) -> f64 {
        let hits = self.hits.load(Ordering::Relaxed);
        let misses = self.misses.load(Ordering::Relaxed);
        let total = hits + misses;
        if total == 0 {
            0.0
        } else {
            hits as f64 / total as f64
        }
    }
}

// ─── Semantic Cache ───────────────────────────────────────────────────────────

pub struct SemanticCache {
    store: DashMap<String, CachedEntry>,
    metrics: Arc<CacheMetrics>,
    capacity: usize,
    ttl: Duration,
    redis: Option<Arc<Mutex<ConnectionManager>>>,
}

impl SemanticCache {
    pub async fn new(redis_url: &str) -> Self {
        Self::with_config_and_redis(MAX_CACHE_CAPACITY, DEFAULT_TTL, redis_url).await
    }

    pub async fn with_config_and_redis(capacity: usize, ttl: Duration, redis_url: &str) -> Self {
        assert!(capacity > 0, "SemanticCache: capacity must be > 0");

        let redis = match redis::Client::open(redis_url) {
            Err(e) => {
                warn!(error = %e, "redis_client_open_failed — RAM-only mode");
                None
            }
            Ok(client) => match ConnectionManager::new(client).await {
                Ok(mgr) => {
                    info!(redis_url = redis_url, "redis_connected");
                    Some(Arc::new(Mutex::new(mgr)))
                }
                Err(e) => {
                    warn!(
                        error = %e,
                        "redis_connection_failed — RAM-only mode. \
                         Restart server once Redis is available."
                    );
                    None
                }
            },
        };

        let cache = Self {
            store: DashMap::with_capacity(capacity),
            metrics: Arc::new(CacheMetrics::default()),
            capacity,
            ttl,
            redis,
        };

        cache.restore_from_redis().await;

        info!(
            capacity = capacity,
            ttl_secs = ttl.as_secs(),
            max_ram_mb = (capacity * 2_200) / 1_048_576,
            "semantic_cache_initialized"
        );

        cache
    }

    pub fn new_in_memory(capacity: usize, ttl: Duration) -> Self {
        assert!(capacity > 0, "SemanticCache: capacity must be > 0");
        Self {
            store: DashMap::with_capacity(capacity),
            metrics: Arc::new(CacheMetrics::default()),
            capacity,
            ttl,
            redis: None,
        }
    }

    // ── Persistence ───────────────────────────────────────────────────────

    async fn restore_from_redis(&self) {
        let redis = match &self.redis {
            Some(r) => Arc::clone(r),
            None => return,
        };

        info!("redis_restore_starting");
        let start = Instant::now();

        let mut conn = redis.lock().await;
        let mut cursor: u64 = 0;
        let mut restored = 0usize;
        let mut skipped = 0usize;

        loop {
            let (next_cursor, keys): (u64, Vec<String>) = match redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(REDIS_SCAN_PATTERN)
                .arg("COUNT")
                .arg(100u64)
                .query_async(&mut *conn)
                .await
            {
                Ok(result) => result,
                Err(e) => {
                    warn!(error = %e, "redis_scan_failed — partial restore");
                    break;
                }
            };

            for key in keys {
                let prompt = key
                    .strip_prefix(REDIS_KEY_PREFIX)
                    .unwrap_or(&key)
                    .to_string();

                match conn.get::<_, Vec<u8>>(&key).await {
                    Err(_) => {
                        skipped += 1;
                        continue;
                    }
                    Ok(bytes) => match bincode::deserialize::<RedisEntry>(&bytes) {
                        Err(_) => {
                            skipped += 1;
                            continue;
                        }
                        Ok(entry) => {
                            let age = entry.age_secs();

                            if age >= self.ttl.as_secs() {
                                skipped += 1;
                                let _ = conn.del::<_, ()>(&key).await;
                                continue;
                            }

                            self.store.insert(
                                prompt,
                                CachedEntry::new(entry.vector, entry.response, age),
                            );
                            restored += 1;
                        }
                    },
                }
            }

            cursor = next_cursor;
            if cursor == 0 {
                break;
            }
        }

        info!(
            restored = restored,
            skipped = skipped,
            elapsed_ms = start.elapsed().as_millis(),
            "redis_restore_complete"
        );
    }

    // ── Public API ────────────────────────────────────────────────────────

    pub fn insert(&self, prompt: String, vector: Vec<f32>, response: String) {
        if self.store.len() >= self.capacity {
            self.evict_lru();
        }

        let fill_ratio = self.store.len() as f64 / self.capacity as f64;
        if fill_ratio >= CAPACITY_WARN_THRESHOLD {
            warn!(
                fill_pct = format!("{:.1}%", fill_ratio * 100.0),
                capacity = self.capacity,
                "semantic_cache_nearing_capacity"
            );
        }

        self.store.insert(
            prompt.clone(),
            CachedEntry {
                vector: vector.clone(),
                response: response.clone(),
                inserted_at: Instant::now(),
                hit_count: 0,
            },
        );
        self.metrics.insertions.fetch_add(1, Ordering::Relaxed);

        if let Some(redis) = self.redis.clone() {
            let key = redis_key(&prompt);
            tokio::spawn(async move {
                let entry = RedisEntry::new(vector, response);
                match bincode::serialize(&entry) {
                    Err(e) => warn!(error = %e, "redis_serialize_failed"),
                    Ok(bytes) => {
                        let mut conn = redis.lock().await;
                        if let Err(e) = conn.set::<_, _, ()>(&key, bytes).await {
                            warn!(error = %e, key = %key, "redis_write_failed");
                        }
                    }
                }
            });
        }

        debug!(
            cache_size = self.store.len(),
            hit_rate = format!("{:.2}%", self.metrics.hit_rate() * 100.0),
            "semantic_cache_inserted"
        );
    }

    pub fn search(&self, query_vector: &[f32]) -> Option<String> {
        let mut best_key: Option<String> = None;
        let mut best_score: f32 = 0.0;

        for entry in self.store.iter() {
            let cached = entry.value();

            if cached.is_expired(self.ttl) {
                self.metrics.expired_hits.fetch_add(1, Ordering::Relaxed);
                continue;
            }

            let score = VectorEngine::cosine_similarity(query_vector, &cached.vector);

            if score >= SIMILARITY_THRESHOLD && score > best_score {
                best_score = score;
                best_key = Some(entry.key().clone());
            }
        }

        if let Some(ref key) = best_key {
            if let Some(mut entry) = self.store.get_mut(key) {
                entry.hit_count += 1;
                let response = entry.response.clone();

                self.metrics.hits.fetch_add(1, Ordering::Relaxed);
                debug!(
                    score = best_score,
                    hit_count = entry.hit_count,
                    hit_rate = format!("{:.2}%", self.metrics.hit_rate() * 100.0),
                    "semantic_cache_hit"
                );
                return Some(response);
            }
        }

        self.metrics.misses.fetch_add(1, Ordering::Relaxed);
        debug!(
            cache_size = self.store.len(),
            hit_rate = format!("{:.2}%", self.metrics.hit_rate() * 100.0),
            "semantic_cache_miss"
        );
        None
    }

    pub fn len(&self) -> usize {
        self.store.len()
    }
    pub fn is_empty(&self) -> bool {
        self.store.is_empty()
    }
    pub fn metrics(&self) -> &CacheMetrics {
        &self.metrics
    }

    // ── Private: LRU Eviction ─────────────────────────────────────────────

    fn evict_lru(&self) {
        let mut entries: Vec<(String, u64)> = self
            .store
            .iter()
            .map(|e| (e.key().clone(), e.value().hit_count))
            .collect();

        entries.sort_unstable_by_key(|(_, count)| *count);

        let keys_to_remove: Vec<String> = entries
            .into_iter()
            .take(EVICTION_BATCH_SIZE)
            .map(|(k, _)| k)
            .collect();

        let mut evicted = 0usize;

        for key in &keys_to_remove {
            if self.store.remove(key).is_some() {
                evicted += 1;
            }
        }

        if let Some(redis) = self.redis.clone() {
            let redis_keys: Vec<String> = keys_to_remove.iter().map(|k| redis_key(k)).collect();

            tokio::spawn(async move {
                let mut conn = redis.lock().await;
                if let Err(e) = conn.del::<_, ()>(redis_keys.as_slice()).await {
                    warn!(error = %e, "redis_bulk_delete_failed");
                }
            });
        }

        self.metrics
            .evictions
            .fetch_add(evicted as u64, Ordering::Relaxed);
        warn!(
            evicted = evicted,
            cache_size = self.store.len(),
            "semantic_cache_lru_eviction"
        );
    }
}
