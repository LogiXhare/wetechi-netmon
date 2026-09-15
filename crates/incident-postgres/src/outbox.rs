//! The transactional outbox's consumer side (ADR 0033): claim a batch under
//! a lease, then mark each message published or failed.
//!
//! - **Clocks.** Every lease and backoff comparison uses PostgreSQL's
//!   `transaction_timestamp()`. Client time is never compared.
//! - **Delivery is at-least-once.** A consumer that delivers a message and
//!   then crashes before [`OutboxConsumer::mark_published`] leaves the row
//!   to be reclaimed when its lease expires. The downstream consumer
//!   de-duplicates on `(aggregate_id, aggregate_version, event_type)`.
//! - **Scope.** The outbox is read across tenants: a consumer such as the
//!   analytics exporter serves the whole platform. Each message carries its
//!   own `tenant_id`.
//! - **Defaults are starting values, not measured ones.** ADR 0033 leaves
//!   the lease, batch size and retry limit to configuration, and asks that
//!   the lease default be informed by the performance-test plan.

use std::time::Duration;

use tokio_postgres::{Client, GenericClient, IsolationLevel, Row};

use crate::error::PersistError;
use crate::retry::RetryPolicy;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutboxPolicy {
    /// The most rows one claim takes.
    pub batch_size: u32,
    /// How long a claim holds its rows before another consumer may reclaim
    /// them.
    pub lease: Duration,
    /// `max_attempts` is the number of failed publishes after which a
    /// message is dead-lettered; the delays are the backoff between them.
    pub retry: RetryPolicy,
}

impl OutboxPolicy {
    pub const fn starting_default() -> Self {
        OutboxPolicy {
            batch_size: 100,
            lease: Duration::from_secs(60),
            retry: RetryPolicy {
                max_attempts: 10,
                base_delay: Duration::from_secs(1),
                max_delay: Duration::from_secs(300),
            },
        }
    }
}

impl Default for OutboxPolicy {
    fn default() -> Self {
        Self::starting_default()
    }
}

/// A message this consumer now holds the lease on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimedMessage {
    pub outbox_id: i64,
    pub tenant_id: String,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub aggregate_version: i64,
    pub event_type: String,
    /// The JSON payload, as text.
    pub payload: String,
    /// Failed publishes before this claim.
    pub attempts: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureOutcome {
    /// The message will be claimable again once its backoff elapses.
    Retrying { attempts: u32 },
    /// The retry limit was reached: the message was copied to
    /// `incident_dead_letter` and will not be claimed again.
    DeadLettered { attempts: u32 },
    /// This consumer no longer holds the lease, so nothing was changed.
    LeaseLost,
}

/// Counts for the `outbox_pending`, `outbox_retries` and `dead_letter_count`
/// metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutboxStats {
    pub pending: i64,
    pub retrying: i64,
    pub unreviewed_dead_letter: i64,
}

/// ADR 0033's lease-aware claim. Selecting and leasing happen in one
/// statement, so in one transaction.
const CLAIM: &str = "\
WITH claimable AS (
    SELECT outbox_id FROM incident_outbox
    WHERE status IN ('pending', 'retrying')
      AND available_at <= transaction_timestamp()
      AND (
        locked_at IS NULL
        OR locked_at + $1::bigint * interval '1 microsecond' <= transaction_timestamp()
      )
    ORDER BY outbox_id
    FOR UPDATE SKIP LOCKED
    LIMIT $2
)
UPDATE incident_outbox AS o
SET locked_at = transaction_timestamp(), locked_by = $3
FROM claimable
WHERE o.outbox_id = claimable.outbox_id
RETURNING o.outbox_id, o.tenant_id, o.aggregate_type, o.aggregate_id,
    o.aggregate_version, o.event_type, o.payload::text AS payload, o.attempts";

const MARK_PUBLISHED: &str = "\
UPDATE incident_outbox
SET status = 'published', published_at = transaction_timestamp(),
    locked_at = NULL, locked_by = NULL, last_error = NULL
WHERE outbox_id = $1 AND locked_by = $2 AND status IN ('pending', 'retrying')";

const LOCK_HELD: &str = "\
SELECT attempts FROM incident_outbox
WHERE outbox_id = $1 AND locked_by = $2 AND status IN ('pending', 'retrying')
FOR UPDATE";

const SCHEDULE_RETRY: &str = "\
UPDATE incident_outbox
SET status = 'retrying', attempts = $2, last_error = $3,
    locked_at = NULL, locked_by = NULL,
    available_at = transaction_timestamp() + $4::bigint * interval '1 microsecond'
WHERE outbox_id = $1";

const COPY_TO_DEAD_LETTER: &str = "\
INSERT INTO incident_dead_letter (
    tenant_id, aggregate_type, aggregate_id, event_type, payload, failure_reason, attempts
)
SELECT tenant_id, aggregate_type, aggregate_id, event_type, payload, $2::text, $3::integer
FROM incident_outbox WHERE outbox_id = $1";

const MARK_DEAD_LETTER: &str = "\
UPDATE incident_outbox
SET status = 'dead_letter', attempts = $2, last_error = $3,
    locked_at = NULL, locked_by = NULL
WHERE outbox_id = $1";

const STATS: &str = "\
SELECT
    (SELECT count(*) FROM incident_outbox WHERE status = 'pending') AS pending,
    (SELECT count(*) FROM incident_outbox WHERE status = 'retrying') AS retrying,
    (SELECT count(*) FROM incident_dead_letter WHERE reviewed_at IS NULL) AS unreviewed_dead_letter";

/// One named consumer. `consumer_id` is recorded in `locked_by`; two
/// processes must not share one.
#[derive(Debug, Clone)]
pub struct OutboxConsumer {
    consumer_id: String,
    policy: OutboxPolicy,
}

impl OutboxConsumer {
    pub fn new(consumer_id: impl Into<String>, policy: OutboxPolicy) -> Self {
        OutboxConsumer {
            consumer_id: consumer_id.into(),
            policy,
        }
    }

    /// Leases up to `batch_size` claimable messages, oldest first. A row is
    /// claimable when it is pending or retrying, its backoff has elapsed,
    /// and it is unleased or its lease has expired. Rows another claimer is
    /// taking at the same moment are skipped, not waited on.
    pub async fn claim(
        &self,
        client: &impl GenericClient,
    ) -> Result<Vec<ClaimedMessage>, PersistError> {
        let lease = micros(self.policy.lease);
        let limit = i64::from(self.policy.batch_size);
        let mut messages = client
            .query(CLAIM, &[&lease, &limit, &self.consumer_id])
            .await?
            .iter()
            .map(claimed_message)
            .collect::<Result<Vec<_>, PersistError>>()?;
        messages.sort_by_key(|message| message.outbox_id);
        Ok(messages)
    }

    /// Marks a message this consumer holds as published. `false` if it no
    /// longer holds it: another consumer reclaimed it after the lease
    /// expired, and that consumer now owns the outcome.
    pub async fn mark_published(
        &self,
        client: &impl GenericClient,
        outbox_id: i64,
    ) -> Result<bool, PersistError> {
        let updated = client
            .execute(MARK_PUBLISHED, &[&outbox_id, &self.consumer_id])
            .await?;
        Ok(updated == 1)
    }

    /// Records one failed publish of a message this consumer holds. At the
    /// retry limit the message is dead-lettered; otherwise it is scheduled
    /// again after the policy's backoff. `attempts` rises once per call,
    /// never per claim.
    pub async fn mark_failed(
        &self,
        client: &mut Client,
        outbox_id: i64,
        error: &str,
    ) -> Result<FailureOutcome, PersistError> {
        let transaction = client
            .build_transaction()
            .isolation_level(IsolationLevel::ReadCommitted)
            .start()
            .await?;
        let Some(row) = transaction
            .query_opt(LOCK_HELD, &[&outbox_id, &self.consumer_id])
            .await?
        else {
            return Ok(FailureOutcome::LeaseLost);
        };
        let previous: i32 = row.try_get("attempts")?;
        let attempts = previous
            .checked_add(1)
            .ok_or_else(|| PersistError::corrupt("attempts", "is at its maximum"))?;
        let attempts_u32 = u32::try_from(attempts)
            .map_err(|_| PersistError::corrupt("attempts", "is negative"))?;

        let outcome = if attempts_u32 >= self.policy.retry.max_attempts.max(1) {
            transaction
                .execute(COPY_TO_DEAD_LETTER, &[&outbox_id, &error, &attempts])
                .await?;
            transaction
                .execute(MARK_DEAD_LETTER, &[&outbox_id, &attempts, &error])
                .await?;
            FailureOutcome::DeadLettered {
                attempts: attempts_u32,
            }
        } else {
            let backoff = micros(self.policy.retry.delay_after(attempts_u32));
            transaction
                .execute(SCHEDULE_RETRY, &[&outbox_id, &attempts, &error, &backoff])
                .await?;
            FailureOutcome::Retrying {
                attempts: attempts_u32,
            }
        };
        transaction.commit().await?;
        Ok(outcome)
    }
}

pub async fn outbox_stats(client: &impl GenericClient) -> Result<OutboxStats, PersistError> {
    let row = client.query_one(STATS, &[]).await?;
    Ok(OutboxStats {
        pending: row.try_get("pending")?,
        retrying: row.try_get("retrying")?,
        unreviewed_dead_letter: row.try_get("unreviewed_dead_letter")?,
    })
}

fn claimed_message(row: &Row) -> Result<ClaimedMessage, PersistError> {
    Ok(ClaimedMessage {
        outbox_id: row.try_get("outbox_id")?,
        tenant_id: row.try_get("tenant_id")?,
        aggregate_type: row.try_get("aggregate_type")?,
        aggregate_id: row.try_get("aggregate_id")?,
        aggregate_version: row.try_get("aggregate_version")?,
        event_type: row.try_get("event_type")?,
        payload: row.try_get("payload")?,
        attempts: row.try_get("attempts")?,
    })
}

/// Microseconds, saturating: no real lease or backoff comes near the limit.
pub(crate) fn micros(duration: Duration) -> i64 {
    i64::try_from(duration.as_micros()).unwrap_or(i64::MAX)
}
