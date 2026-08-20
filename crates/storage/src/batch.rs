//! A bounded batch queue: accumulate rows, flush on size-or-time
//! threshold. Pure state — no I/O — so it is fully unit-testable without
//! a ClickHouse server. See
//! docs/architecture/decisions/0005-clickhouse-batching-and-retry.md.

use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchConfig {
    pub max_rows: usize,
    pub max_interval: Duration,
}

impl Default for BatchConfig {
    fn default() -> Self {
        BatchConfig {
            max_rows: 10_000,
            max_interval: Duration::from_secs(5),
        }
    }
}

pub struct BatchQueue<T> {
    config: BatchConfig,
    rows: Vec<T>,
    window_start: Instant,
}

impl<T> BatchQueue<T> {
    pub fn new(config: BatchConfig, now: Instant) -> Self {
        BatchQueue {
            config,
            rows: Vec::new(),
            window_start: now,
        }
    }

    pub fn push(&mut self, row: T) {
        self.rows.push(row);
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Whether the queue should be flushed right now: it has reached
    /// `max_rows`, or `max_interval` has elapsed since the window
    /// started (and there is at least one row — an empty queue is never
    /// worth flushing).
    pub fn should_flush(&self, now: Instant) -> bool {
        if self.rows.is_empty() {
            return false;
        }
        self.rows.len() >= self.config.max_rows
            || now.duration_since(self.window_start) >= self.config.max_interval
    }

    /// Takes every accumulated row and resets the window, regardless of
    /// whether `should_flush` would currently return `true` — the caller
    /// decides when to flush; this just performs it.
    pub fn take(&mut self, now: Instant) -> Vec<T> {
        self.window_start = now;
        std::mem::take(&mut self.rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn does_not_flush_an_empty_queue() {
        let now = Instant::now();
        let queue: BatchQueue<u32> = BatchQueue::new(BatchConfig::default(), now);
        assert!(!queue.should_flush(now + Duration::from_secs(100)));
    }

    #[test]
    fn flushes_when_max_rows_reached() {
        let now = Instant::now();
        let config = BatchConfig {
            max_rows: 3,
            max_interval: Duration::from_secs(3600),
        };
        let mut queue = BatchQueue::new(config, now);
        queue.push(1);
        queue.push(2);
        assert!(!queue.should_flush(now));
        queue.push(3);
        assert!(queue.should_flush(now));
    }

    #[test]
    fn flushes_when_max_interval_elapses() {
        let now = Instant::now();
        let config = BatchConfig {
            max_rows: 1_000_000,
            max_interval: Duration::from_secs(5),
        };
        let mut queue = BatchQueue::new(config, now);
        queue.push(1);
        assert!(!queue.should_flush(now + Duration::from_secs(1)));
        assert!(queue.should_flush(now + Duration::from_secs(6)));
    }

    #[test]
    fn take_resets_the_window_and_returns_all_rows() {
        let now = Instant::now();
        let mut queue = BatchQueue::new(BatchConfig::default(), now);
        queue.push(1);
        queue.push(2);
        let taken = queue.take(now + Duration::from_secs(10));
        assert_eq!(taken, vec![1, 2]);
        assert!(queue.is_empty());
        // Window reset — should not immediately re-flush on time alone.
        assert!(!queue.should_flush(now + Duration::from_secs(10)));
    }
}
