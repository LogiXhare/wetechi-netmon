//! Idempotency: fingerprinted keys, per ADR 0016.
//!
//! Owner decision: the fingerprint contract must not be
//! `std::collections::hash_map::DefaultHasher`. `DefaultHasher`'s output
//! is explicitly not guaranteed stable across Rust releases, which the
//! detector's own `detection_id` scheme (ADR 0009) accepts because that
//! identifier is ephemeral and process-local. An idempotency fingerprint
//! is compared against a *previously stored* value, potentially minutes
//! or hours later — a stability assumption `DefaultHasher` does not make
//! and this crate must not lean on.
//!
//! [`RequestFingerprint`] instead holds the canonical serialized bytes of
//! the command directly and compares them by equality — not a hash of
//! them. This is strictly more precise than hashing (no collision risk
//! to reason about at all) and adds no dependency: canonicalisation uses
//! `serde_json`, already in this crate's manifest, with `BTreeMap`
//! preferred over `HashMap` anywhere a command carries a map, so field
//! order is deterministic. No `Debug` output is ever used as a
//! fingerprint — `Debug` formatting is not a stable contract either.

use std::collections::HashMap;

use serde::Serialize;

use crate::correlation::TenantId;
use crate::error::IncidentError;
use crate::id::IncidentId;

pub const IDEMPOTENCY_KEY_MIN_LEN: usize = 16;
pub const IDEMPOTENCY_KEY_MAX_LEN: usize = 255;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdempotencyKey(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum IdempotencyKeyError {
    #[error("idempotency key length {0} is outside the accepted 16-255 character range")]
    InvalidLength(usize),
}

impl IdempotencyKey {
    pub fn new(value: impl Into<String>) -> Result<Self, IdempotencyKeyError> {
        let value = value.into();
        if !(IDEMPOTENCY_KEY_MIN_LEN..=IDEMPOTENCY_KEY_MAX_LEN).contains(&value.len()) {
            return Err(IdempotencyKeyError::InvalidLength(value.len()));
        }
        Ok(IdempotencyKey(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Canonical serialized bytes of a command, compared directly — never
/// hashed, never a `Debug` string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestFingerprint(Vec<u8>);

impl RequestFingerprint {
    /// Builds a fingerprint from any canonically serializable command.
    /// Callers must use `BTreeMap`, not `HashMap`, for any map-shaped
    /// field on the value passed here, or two logically identical
    /// commands could produce different byte sequences.
    pub fn of<T: Serialize>(value: &T) -> Self {
        let bytes = serde_json::to_vec(value).expect("command must serialize canonically");
        RequestFingerprint(bytes)
    }
}

/// What a previously completed command produced, stored so a replay can
/// return it without re-executing anything.
///
/// `Failed` carries the actual [`IncidentError`] the first attempt
/// returned — not a single collapsed `Denied` marker — so a replay
/// reproduces the same error category the caller originally saw:
/// `InvalidTransition` replays as `InvalidTransition`, `VersionConflict`
/// as `VersionConflict`, `Unauthorized` as `Unauthorized`, and so on. A
/// deterministic domain outcome is safe to replay exactly. What must
/// never be stored here is a transient or injected failure —
/// [`crate::unit_of_work::IncidentUnitOfWork::handle_command`] does not
/// call [`IdempotencyStore::record`] for
/// [`IncidentError::InternalInvariantViolation`], so that class of
/// failure never poisons a key and a retry under the same key can still
/// succeed once the underlying condition clears.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredOutcome {
    Mutated {
        incident_id: IncidentId,
        version: u64,
    },
    Failed(IncidentError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IdempotencyRecord {
    fingerprint: RequestFingerprint,
    outcome: StoredOutcome,
}

/// What the idempotency store decided for one `(tenant, key, fingerprint)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdempotencyCheck {
    /// No record for this key; the caller should process normally.
    New,
    /// Same key, same fingerprint: replay this stored outcome without
    /// mutating anything.
    Replay(StoredOutcome),
    /// Same key, a **different** fingerprint. Returning the old outcome
    /// here would tell the caller their new request succeeded when it
    /// was never applied — the only safe answer is a conflict.
    Conflict,
}

#[derive(Default)]
pub struct IdempotencyStore {
    records: HashMap<(TenantId, IdempotencyKey), IdempotencyRecord>,
}

impl IdempotencyStore {
    pub fn new() -> Self {
        IdempotencyStore::default()
    }

    pub fn check(
        &self,
        tenant: &TenantId,
        key: &IdempotencyKey,
        fingerprint: &RequestFingerprint,
    ) -> IdempotencyCheck {
        match self.records.get(&(tenant.clone(), key.clone())) {
            None => IdempotencyCheck::New,
            Some(record) if &record.fingerprint == fingerprint => {
                IdempotencyCheck::Replay(record.outcome.clone())
            }
            Some(_) => IdempotencyCheck::Conflict,
        }
    }

    pub fn record(
        &mut self,
        tenant: TenantId,
        key: IdempotencyKey,
        fingerprint: RequestFingerprint,
        outcome: StoredOutcome,
    ) {
        self.records.insert(
            (tenant, key),
            IdempotencyRecord {
                fingerprint,
                outcome,
            },
        );
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[derive(Serialize)]
    struct FakeCommand {
        field: String,
    }

    #[test]
    fn identical_canonical_commands_produce_equal_fingerprints() {
        let a = RequestFingerprint::of(&FakeCommand {
            field: "x".to_string(),
        });
        let b = RequestFingerprint::of(&FakeCommand {
            field: "x".to_string(),
        });
        assert_eq!(a, b);
    }

    #[test]
    fn different_commands_produce_different_fingerprints() {
        let a = RequestFingerprint::of(&FakeCommand {
            field: "x".to_string(),
        });
        let b = RequestFingerprint::of(&FakeCommand {
            field: "y".to_string(),
        });
        assert_ne!(a, b);
    }

    #[test]
    fn same_key_same_fingerprint_replays() {
        let mut store = IdempotencyStore::new();
        let tenant = TenantId::new("acme");
        let key = IdempotencyKey::new("a".repeat(20)).unwrap();
        let fp = RequestFingerprint::of(&FakeCommand {
            field: "x".to_string(),
        });
        let outcome = StoredOutcome::Mutated {
            incident_id: IncidentId::from_bytes([1; 16]),
            version: 2,
        };
        store.record(tenant.clone(), key.clone(), fp.clone(), outcome.clone());
        assert_eq!(
            store.check(&tenant, &key, &fp),
            IdempotencyCheck::Replay(outcome)
        );
    }

    #[test]
    fn same_key_different_fingerprint_conflicts() {
        let mut store = IdempotencyStore::new();
        let tenant = TenantId::new("acme");
        let key = IdempotencyKey::new("a".repeat(20)).unwrap();
        let fp1 = RequestFingerprint::of(&FakeCommand {
            field: "x".to_string(),
        });
        let fp2 = RequestFingerprint::of(&FakeCommand {
            field: "y".to_string(),
        });
        store.record(
            tenant.clone(),
            key.clone(),
            fp1,
            StoredOutcome::Failed(IncidentError::Unauthorized),
        );
        assert_eq!(store.check(&tenant, &key, &fp2), IdempotencyCheck::Conflict);
    }

    #[test]
    fn a_new_key_is_new() {
        let store = IdempotencyStore::new();
        let tenant = TenantId::new("acme");
        let key = IdempotencyKey::new("a".repeat(20)).unwrap();
        let fp = RequestFingerprint::of(&FakeCommand {
            field: "x".to_string(),
        });
        assert_eq!(store.check(&tenant, &key, &fp), IdempotencyCheck::New);
    }

    #[test]
    fn keys_outside_the_length_bound_are_rejected() {
        assert!(IdempotencyKey::new("short").is_err());
        assert!(IdempotencyKey::new("a".repeat(300)).is_err());
        assert!(IdempotencyKey::new("a".repeat(16)).is_ok());
        assert!(IdempotencyKey::new("a".repeat(255)).is_ok());
    }

    #[test]
    fn different_tenants_do_not_share_a_record() {
        let mut store = IdempotencyStore::new();
        let key = IdempotencyKey::new("a".repeat(20)).unwrap();
        let fp = RequestFingerprint::of(&FakeCommand {
            field: "x".to_string(),
        });
        store.record(
            TenantId::new("acme"),
            key.clone(),
            fp.clone(),
            StoredOutcome::Failed(IncidentError::Unauthorized),
        );
        assert_eq!(
            store.check(&TenantId::new("globex"), &key, &fp),
            IdempotencyCheck::New
        );
    }
}
