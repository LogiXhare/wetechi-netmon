//! Ties [`crate::batch::BatchQueue`] and [`crate::retry::RetryQueue`] to
//! a real ClickHouse connection. The batching/retry *decisions* are pure
//! and unit-tested (see those modules); this module is the thin,
//! I/O-performing layer around them, covered by an integration test that
//! is skipped (not faked) when no ClickHouse server is reachable — see
//! docs/integrations/clickhouse.md.

use clickhouse::{Client, Row};
use serde::Serialize;
use std::time::Instant;

use crate::batch::{BatchConfig, BatchQueue};
use crate::retry::{EnqueueOutcome, RequeueOutcome, RetryConfig, RetryQueue};
use crate::schema::CREATE_TABLE_STATEMENTS;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FlushReport {
    pub rows_written: usize,
    pub write_failures: u32,
    pub retry_queue_dropped: u32,
    pub permanently_dropped_batches: u32,
}

pub struct ClickHouseWriter<T> {
    client: Client,
    table: &'static str,
    batch: BatchQueue<T>,
    retry: RetryQueue<T>,
}

impl<T> ClickHouseWriter<T>
where
    T: Row + Serialize + Clone + Send + Sync + 'static,
{
    pub fn new(
        client: Client,
        table: &'static str,
        batch_config: BatchConfig,
        retry_config: RetryConfig,
        now: Instant,
    ) -> Self {
        ClickHouseWriter {
            client,
            table,
            batch: BatchQueue::new(batch_config, now),
            retry: RetryQueue::new(retry_config),
        }
    }

    pub fn push(&mut self, row: T) {
        self.batch.push(row);
    }

    pub fn pending_rows(&self) -> usize {
        self.batch.len()
    }

    pub fn pending_retry_batches(&self) -> usize {
        self.retry.len()
    }

    /// Flushes the batch queue if due, and attempts any retry-queue
    /// batches whose backoff has elapsed. Real network I/O — see the
    /// module docs about integration-test coverage.
    pub async fn tick(&mut self, now: Instant) -> FlushReport {
        let mut report = FlushReport::default();

        if self.batch.should_flush(now) {
            let rows = self.batch.take(now);
            self.attempt_write(rows, 0, now, &mut report).await;
        }

        for (rows, attempts) in self.retry.take_due(now) {
            self.attempt_write(rows, attempts, now, &mut report).await;
        }

        report
    }

    async fn attempt_write(
        &mut self,
        rows: Vec<T>,
        previous_attempts: u32,
        now: Instant,
        report: &mut FlushReport,
    ) {
        if rows.is_empty() {
            return;
        }
        match self.write_rows(&rows).await {
            Ok(()) => {
                report.rows_written += rows.len();
            }
            Err(err) => {
                report.write_failures += 1;
                tracing::warn!(table = self.table, error = %err, rows = rows.len(), "ClickHouse write failed, queueing for retry");
                let outcome = self
                    .retry
                    .requeue_after_failure(rows, previous_attempts, now);
                match outcome {
                    RequeueOutcome::Requeued(EnqueueOutcome::EnqueuedByDroppingOldest) => {
                        report.retry_queue_dropped += 1;
                    }
                    RequeueOutcome::PermanentlyDropped => {
                        report.permanently_dropped_batches += 1;
                    }
                    RequeueOutcome::Requeued(EnqueueOutcome::Enqueued) => {}
                }
            }
        }
    }

    async fn write_rows(&self, rows: &[T]) -> Result<(), clickhouse::error::Error> {
        let mut insert = self.client.insert(self.table)?;
        for row in rows {
            insert.write(row).await?;
        }
        insert.end().await
    }
}

/// Runs every table's `CREATE TABLE IF NOT EXISTS` statement. Idempotent
/// — safe to call on every startup, not just first install (Phase 3
/// objective 10 "migrations").
pub async fn run_migrations(client: &Client) -> Result<(), clickhouse::error::Error> {
    for (name, ddl) in CREATE_TABLE_STATEMENTS {
        tracing::info!(table = name, "ensuring ClickHouse table exists");
        client.query(ddl).execute().await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // These exercise only the parts of ClickHouseWriter that don't need
    // a live server — pending-count bookkeeping. Actual write behavior
    // is covered by the integration test in tests/clickhouse_integration.rs,
    // which skips cleanly when CLICKHOUSE_TEST_URL is unset.

    #[test]
    fn batch_and_retry_config_defaults_are_sane() {
        // A smoke check that the default configs at least construct and
        // have non-zero limits — regression guard against an accidental
        // `Default` that would make the writer never flush or never
        // retry.
        let batch = BatchConfig::default();
        assert!(batch.max_rows > 0);
        let retry = RetryConfig::default();
        assert!(retry.max_pending_batches > 0);
        assert!(retry.max_attempts > 0);
    }
}
