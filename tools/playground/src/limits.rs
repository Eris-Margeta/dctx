// SPDX-License-Identifier: Apache-2.0
//! Abuse controls: a per-client token bucket, a global run gate with a bounded queue,
//! an identical-request result cache and counters. Nothing here is written to disk, and no
//! client address outlives its bucket.

use std::collections::{HashMap, VecDeque};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// Per-client token bucket: `rate` tokens per minute, at most `burst` saved.
pub struct Buckets {
    rate_per_min: f64,
    burst: f64,
    state: Mutex<HashMap<String, (f64, Instant)>>,
}

impl Buckets {
    pub fn new(rate_per_min: u32, burst: u32) -> Self {
        Buckets { rate_per_min: f64::from(rate_per_min), burst: f64::from(burst.max(1)), state: Mutex::new(HashMap::new()) }
    }

    /// Takes one token for `client`; `Err(seconds)` says when the next one is due.
    pub fn take(&self, client: &str, now: Instant) -> Result<(), u64> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.len() > 10_000 {
            // Forget full buckets; a full bucket is the same as a new one.
            let (rate, burst) = (self.rate_per_min, self.burst);
            state.retain(|_, (tokens, at)| *tokens + now.duration_since(*at).as_secs_f64() * rate / 60.0 < burst);
        }
        let entry = state.entry(client.to_string()).or_insert((self.burst, now));
        let refill = now.duration_since(entry.1).as_secs_f64() * self.rate_per_min / 60.0;
        entry.0 = (entry.0 + refill).min(self.burst);
        entry.1 = now;
        if entry.0 >= 1.0 {
            entry.0 -= 1.0;
            Ok(())
        } else {
            let wait = (1.0 - entry.0) * 60.0 / self.rate_per_min.max(0.001);
            Err(wait.ceil().max(1.0) as u64)
        }
    }
}

/// At most `running` concurrent runs and `queue` waiting ones; more are refused.
pub struct Gate {
    running: usize,
    queue: usize,
    state: Mutex<(usize, usize)>,
    turn: Condvar,
}

/// Releases the run slot on drop.
pub struct Slot<'a>(&'a Gate);

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        let mut state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
        state.0 -= 1;
        self.0.turn.notify_one();
    }
}

impl Gate {
    pub fn new(running: usize, queue: usize) -> Self {
        Gate { running: running.max(1), queue, state: Mutex::new((0, 0)), turn: Condvar::new() }
    }

    /// Waits for a slot, or returns `None` at once when the queue is full.
    pub fn enter(&self) -> Option<Slot<'_>> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.0 >= self.running {
            if state.1 >= self.queue {
                return None;
            }
            state.1 += 1;
            while state.0 >= self.running {
                state = self.turn.wait(state).unwrap_or_else(|e| e.into_inner());
            }
            state.1 -= 1;
        }
        state.0 += 1;
        Some(Slot(self))
    }

    /// (running, waiting) now.
    pub fn load(&self) -> (usize, usize) {
        *self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// LRU result cache keyed by the request digest, entries expire after `ttl`.
pub struct Cache<V: Clone> {
    capacity: usize,
    ttl: Duration,
    state: Mutex<(HashMap<String, (Instant, V)>, VecDeque<String>)>,
}

impl<V: Clone> Cache<V> {
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        Cache { capacity, ttl, state: Mutex::new((HashMap::new(), VecDeque::new())) }
    }

    pub fn get(&self, key: &str, now: Instant) -> Option<V> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let (map, order) = &mut *state;
        match map.get(key) {
            Some((at, value)) if now.duration_since(*at) < self.ttl => {
                let value = value.clone();
                order.retain(|k| k != key);
                order.push_back(key.to_string());
                Some(value)
            }
            Some(_) => {
                map.remove(key);
                order.retain(|k| k != key);
                None
            }
            None => None,
        }
    }

    pub fn put(&self, key: &str, value: V, now: Instant) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let (map, order) = &mut *state;
        if map.insert(key.to_string(), (now, value)).is_some() {
            order.retain(|k| k != key);
        }
        order.push_back(key.to_string());
        while order.len() > self.capacity {
            if let Some(old) = order.pop_front() {
                map.remove(&old);
            }
        }
    }

    pub fn len(&self) -> usize {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).0.len()
    }
}

/// Counters for `GET /api/v1/metrics.json`. No addresses, no request content.
#[derive(Default)]
pub struct Metrics {
    state: Mutex<MetricsState>,
}

#[derive(Default)]
struct MetricsState {
    runs: u64,
    cache_hits: u64,
    timeouts: u64,
    rejected: Vec<(String, u64)>,
    durations: VecDeque<u64>,
}

impl Metrics {
    pub fn run(&self, ms: u64, timed_out: bool) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        s.runs += 1;
        s.timeouts += u64::from(timed_out);
        s.durations.push_back(ms);
        if s.durations.len() > 256 {
            s.durations.pop_front();
        }
    }

    pub fn hit(&self) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).cache_hits += 1;
    }

    pub fn reject(&self, reason: &str) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        match s.rejected.iter_mut().find(|(r, _)| r == reason) {
            Some((_, n)) => *n += 1,
            None => s.rejected.push((reason.to_string(), 1)),
        }
    }

    /// (runs, cache hits, timeouts, rejected by reason, median run ms).
    pub fn snapshot(&self) -> (u64, u64, u64, Vec<(String, u64)>, u64) {
        let s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut sorted: Vec<u64> = s.durations.iter().copied().collect();
        sorted.sort_unstable();
        let p50 = sorted.get(sorted.len() / 2).copied().unwrap_or(0);
        let mut rejected = s.rejected.clone();
        rejected.sort();
        (s.runs, s.cache_hits, s.timeouts, rejected, p50)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn bucket_allows_the_burst_then_refills() {
        let buckets = Buckets::new(20, 5);
        let t0 = Instant::now();
        for _ in 0..5 {
            assert!(buckets.take("a", t0).is_ok());
        }
        assert_eq!(buckets.take("a", t0), Err(3));
        assert!(buckets.take("b", t0).is_ok(), "clients are independent");
        assert!(buckets.take("a", t0 + Duration::from_secs(3)).is_ok(), "one token every 3 s");
        assert!(buckets.take("a", t0 + Duration::from_secs(3)).is_err());
    }

    #[test]
    fn gate_refuses_when_the_queue_is_full() {
        let gate = Arc::new(Gate::new(1, 1));
        let first = gate.enter().unwrap();
        let waiter = {
            let gate = Arc::clone(&gate);
            std::thread::spawn(move || gate.enter().map(|_| ()).is_some())
        };
        while gate.load().1 == 0 {
            std::thread::yield_now();
        }
        assert!(gate.enter().is_none(), "one running and one waiting is the limit");
        drop(first);
        assert!(waiter.join().unwrap());
        assert_eq!(gate.load(), (0, 0));
    }

    #[test]
    fn cache_evicts_oldest_and_expires() {
        let cache = Cache::new(2, Duration::from_secs(60));
        let t0 = Instant::now();
        cache.put("a", 1, t0);
        cache.put("b", 2, t0);
        assert_eq!(cache.get("a", t0), Some(1));
        cache.put("c", 3, t0);
        assert_eq!(cache.get("b", t0), None, "b was least recently used");
        assert_eq!(cache.get("a", t0 + Duration::from_secs(61)), None, "expired");
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn metrics_median() {
        let metrics = Metrics::default();
        for ms in [5, 1, 9] {
            metrics.run(ms, false);
        }
        metrics.reject("grammar");
        metrics.reject("grammar");
        let (runs, _, _, rejected, p50) = metrics.snapshot();
        assert_eq!((runs, p50), (3, 5));
        assert_eq!(rejected, vec![("grammar".to_string(), 2)]);
    }
}
