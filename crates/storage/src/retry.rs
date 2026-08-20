//! A bounded retry queue with exponential backoff and
//! drop-oldest-on-overflow. See
//! docs/architecture/decisions/0005-clickhouse-batching-and-retry.md for
//! why dropping the oldest (not newest) pending batch was chosen, and
//! why this is bounded rather than a durable/unbounded queue.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryConfig {
    pub max_pending_batches: usize,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
    pub max_attempts: u32,
}

impl Default for RetryConfig {
    fn default() -> Self {
        RetryConfig {
            max_pending_batches: 100,
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(60),
            max_attempts: 5,
        }
    }
}

struct PendingBatch<T> {
    rows: Vec<T>,
    attempts: u32,
    next_attempt_at: Instant,
}

/// Outcome of pushing a failed batch into the retry queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnqueueOutcome {
    Enqueued,
    /// The queue was full; the oldest pending batch was dropped to make
    /// room. Callers must count this via a metric
    /// (`clickhouse_retry_queue_dropped_total`) — it is real data loss.
    EnqueuedByDroppingOldest,
}

pub struct RetryQueue<T> {
    config: RetryConfig,
    pending: VecDeque<PendingBatch<T>>,
}

impl<T> RetryQueue<T> {
    pub fn new(config: RetryConfig) -> Self {
        RetryQueue {
            config,
            pending: VecDeque::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Enqueues a batch that just failed to write, scheduling its first
    /// retry attempt using `initial_backoff`.
    pub fn enqueue(&mut self, rows: Vec<T>, now: Instant) -> EnqueueOutcome {
        let mut outcome = EnqueueOutcome::Enqueued;
        if self.pending.len() >= self.config.max_pending_batches {
            self.pending.pop_front(); // drop the oldest
            outcome = EnqueueOutcome::EnqueuedByDroppingOldest;
        }
        self.pending.push_back(PendingBatch {
            rows,
            attempts: 0,
            next_attempt_at: now + self.config.initial_backoff,
        });
        outcome
    }

    /// Takes every batch whose backoff has elapsed and is due for retry
    /// now, removing them from the queue, paired with how many attempts
    /// each has already made. The caller attempts each one and calls
    /// [`RetryQueue::requeue_after_failure`] with that attempt count (not
    /// a guessed constant) or simply drops the batch on success.
    pub fn take_due(&mut self, now: Instant) -> Vec<(Vec<T>, u32)> {
        let mut due = Vec::new();
        let mut remaining = VecDeque::new();
        for batch in self.pending.drain(..) {
            if batch.next_attempt_at <= now {
                due.push((batch.rows, batch.attempts));
            } else {
                remaining.push_back(batch);
            }
        }
        self.pending = remaining;
        due
    }

    /// Re-enqueues a batch that failed again, doubling its backoff (capped
    /// at `max_backoff`). Returns `None` (the batch is permanently
    /// dropped, not re-queued) once `max_attempts` is exceeded — an
    /// unbounded retry count is its own resource-exhaustion risk.
    pub fn requeue_after_failure(
        &mut self,
        rows: Vec<T>,
        previous_attempts: u32,
        now: Instant,
    ) -> RequeueOutcome {
        let attempts = previous_attempts + 1;
        if attempts >= self.config.max_attempts {
            return RequeueOutcome::PermanentlyDropped;
        }
        let backoff = self
            .config
            .initial_backoff
            .saturating_mul(2u32.saturating_pow(attempts))
            .min(self.config.max_backoff);
        let outcome = if self.pending.len() >= self.config.max_pending_batches {
            self.pending.pop_front();
            EnqueueOutcome::EnqueuedByDroppingOldest
        } else {
            EnqueueOutcome::Enqueued
        };
        self.pending.push_back(PendingBatch {
            rows,
            attempts,
            next_attempt_at: now + backoff,
        });
        RequeueOutcome::Requeued(outcome)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequeueOutcome {
    Requeued(EnqueueOutcome),
    PermanentlyDropped,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enqueue_and_take_due_respects_backoff() {
        let now = Instant::now();
        let mut queue: RetryQueue<u32> = RetryQueue::new(RetryConfig {
            initial_backoff: Duration::from_secs(10),
            ..Default::default()
        });
        queue.enqueue(vec![1, 2], now);
        assert!(queue.take_due(now).is_empty(), "not due yet");
        let due = queue.take_due(now + Duration::from_secs(11));
        assert_eq!(due, vec![(vec![1, 2], 0)]);
        assert!(queue.is_empty());
    }

    #[test]
    fn overflow_drops_the_oldest_batch_not_the_newest() {
        let mut queue: RetryQueue<u32> = RetryQueue::new(RetryConfig {
            max_pending_batches: 2,
            ..Default::default()
        });
        let now = Instant::now();
        queue.enqueue(vec![1], now);
        queue.enqueue(vec![2], now);
        let outcome = queue.enqueue(vec![3], now);
        assert_eq!(outcome, EnqueueOutcome::EnqueuedByDroppingOldest);
        assert_eq!(queue.len(), 2);

        // The batch containing `1` (oldest) should be gone; `2` and `3` remain.
        let due = queue.take_due(now + Duration::from_secs(3600));
        assert_eq!(due, vec![(vec![2], 0), (vec![3], 0)]);
    }

    #[test]
    fn backoff_doubles_on_repeated_failure() {
        let mut queue: RetryQueue<u32> = RetryQueue::new(RetryConfig {
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(3600),
            max_attempts: 10,
            ..Default::default()
        });
        let now = Instant::now();
        let outcome = queue.requeue_after_failure(vec![1], 0, now);
        assert!(matches!(outcome, RequeueOutcome::Requeued(_)));
        // attempts=1 => backoff = initial * 2^1 = 2s
        assert!(queue.take_due(now + Duration::from_millis(1900)).is_empty());
        assert!(!queue.take_due(now + Duration::from_secs(3)).is_empty());
    }

    #[test]
    fn backoff_is_capped_at_max_backoff() {
        let mut queue: RetryQueue<u32> = RetryQueue::new(RetryConfig {
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(5),
            max_attempts: 20,
            ..Default::default()
        });
        let now = Instant::now();
        // A very high previous_attempts should still cap at max_backoff,
        // not overflow or grow unbounded.
        queue.requeue_after_failure(vec![1], 15, now);
        let due = queue.take_due(now + Duration::from_secs(5) + Duration::from_millis(1));
        assert!(!due.is_empty());
    }

    #[test]
    fn permanently_drops_after_max_attempts() {
        let mut queue: RetryQueue<u32> = RetryQueue::new(RetryConfig {
            max_attempts: 3,
            ..Default::default()
        });
        let now = Instant::now();
        let outcome = queue.requeue_after_failure(vec![1], 2, now); // attempts becomes 3
        assert_eq!(outcome, RequeueOutcome::PermanentlyDropped);
        assert!(queue.is_empty());
    }
}
