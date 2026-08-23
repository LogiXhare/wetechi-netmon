//! The human-readable incident number.
//!
//! **Provisional.** [FU-24](../../../docs/architecture/decisions/0013-incident-identity.md)
//! — whether the sequence resets each year or runs continuously per
//! tenant — is explicitly not decided. This module implements a
//! continuous per-tenant sequence, which is the smaller commitment: it
//! is a strict subset of "start a new sequence every year" (a
//! continuous scheme can always be *segmented* into per-year buckets
//! later without renumbering anything already issued, whereas the
//! reverse — collapsing per-year buckets back into one sequence —
//! cannot be done without either renumbering or leaving a permanent
//! gap). The display form still carries the allocation year for
//! readability, but callers must not parse an [`IncidentNumber`] or rely
//! on its exact shape: [`IncidentNumber::as_str`] is the only stable
//! contract, and the string it returns is documented as provisional.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// A bounded, opaque, tenant-scoped display value.
///
/// Never derived from and never substituted for an [`crate::id::IncidentId`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct IncidentNumber(String);

/// The bound on the formatted number's length. Generous enough for the
/// documented shape (`WNM-YYYY-NNNNNN`, 15 chars) plus the seven-digit
/// overflow the domain model allows without wrapping.
pub const INCIDENT_NUMBER_MAX_LEN: usize = 32;

impl IncidentNumber {
    /// Only [`NumberAllocator`] implementations in this crate call this;
    /// it is `pub(crate)` so a caller cannot mint an unallocated number.
    pub(crate) fn new_unchecked(value: String) -> Self {
        debug_assert!(value.len() <= INCIDENT_NUMBER_MAX_LEN);
        IncidentNumber(value)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for IncidentNumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The ADR-0017 Community extension point: how a tenant's next incident
/// number is produced. Enterprise may substitute a custom format; the
/// Community implementation in this module is complete and is what 5A
/// tests against.
pub trait NumberAllocator: Send + Sync {
    /// Allocates the next number for `tenant`, tagging it with
    /// `allocation_year` for display. Allocation must be effectively
    /// atomic per tenant — two calls for the same tenant must never
    /// return the same number.
    fn allocate(&self, tenant: &str, allocation_year: u32) -> IncidentNumber;
}

/// A per-tenant continuous counter, held in memory.
///
/// This is the Community implementation. It has no persistence — a
/// process restart resets every tenant's counter to zero, which is
/// acceptable in 5A because nothing durable exists yet to disagree with
/// it; Milestone 5B's PostgreSQL sequence replaces the counter without
/// changing this trait.
pub struct InMemoryNumberAllocator {
    counters: Mutex<HashMap<String, u64>>,
}

impl InMemoryNumberAllocator {
    pub fn new() -> Self {
        InMemoryNumberAllocator {
            counters: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryNumberAllocator {
    fn default() -> Self {
        Self::new()
    }
}

impl NumberAllocator for InMemoryNumberAllocator {
    fn allocate(&self, tenant: &str, allocation_year: u32) -> IncidentNumber {
        let mut counters = self.counters.lock().expect("number allocator poisoned");
        let next = counters.entry(tenant.to_string()).or_insert(0);
        *next += 1;
        let seq = *next;
        drop(counters);
        let formatted = if seq <= 999_999 {
            format!("WNM-{allocation_year}-{seq:06}")
        } else {
            // Deliberately not wrapped: overflowing to seven digits is
            // uglier than silently reusing a number, and a tenant
            // exceeding 999,999 incidents in the counter's lifetime has a
            // different problem than formatting.
            format!("WNM-{allocation_year}-{seq}")
        };
        IncidentNumber::new_unchecked(formatted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocates_sequentially_per_tenant() {
        let allocator = InMemoryNumberAllocator::new();
        let first = allocator.allocate("acme", 2026);
        let second = allocator.allocate("acme", 2026);
        assert_eq!(first.as_str(), "WNM-2026-000001");
        assert_eq!(second.as_str(), "WNM-2026-000002");
    }

    #[test]
    fn tenants_do_not_share_a_counter() {
        let allocator = InMemoryNumberAllocator::new();
        allocator.allocate("acme", 2026);
        allocator.allocate("acme", 2026);
        let other = allocator.allocate("globex", 2026);
        assert_eq!(other.as_str(), "WNM-2026-000001");
    }

    #[test]
    fn the_sequence_is_continuous_across_a_year_boundary() {
        // Documents the provisional decision explicitly: allocating with
        // a different `allocation_year` does not reset the counter.
        let allocator = InMemoryNumberAllocator::new();
        let first = allocator.allocate("acme", 2026);
        let second = allocator.allocate("acme", 2027);
        assert_eq!(first.as_str(), "WNM-2026-000001");
        assert_eq!(second.as_str(), "WNM-2027-000002");
    }

    #[test]
    fn never_produces_a_duplicate_number_across_many_allocations() {
        let allocator = InMemoryNumberAllocator::new();
        let numbers: std::collections::BTreeSet<IncidentNumber> =
            (0..500).map(|_| allocator.allocate("acme", 2026)).collect();
        assert_eq!(numbers.len(), 500);
    }
}
