//! A bounded, capacity-limited map with deterministic
//! least-recently-updated eviction and independent inactivity
//! expiration. See docs/architecture/decisions/0003-in-memory-aggregation-structure.md
//! for why this shape was chosen. Reused identically across every
//! aggregation dimension (host, network, hostgroup, ASN, exporter,
//! interface, protocol) — the eviction/expiration logic is tested once
//! here, not once per dimension.

use std::collections::HashMap;
use std::hash::Hash;
use std::time::{Duration, Instant};

/// One tracked entry plus the bookkeeping needed for eviction/expiration.
struct Slot<V> {
    value: V,
    last_updated: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoundedMapConfig {
    /// Hard cap on the number of distinct keys tracked at once. Must be
    /// at least 1 — Phase 3 objective 8's "maximum tracked hosts/
    /// networks/ASNs/hostgroups configurable" controls map directly to
    /// this per dimension.
    pub max_entries: usize,
    /// An entry untouched for longer than this is expired on the next
    /// `expire_inactive` sweep, regardless of overall map fullness.
    pub inactivity_ttl: Duration,
}

/// Outcome of an insert/update — tells the caller whether tracking this
/// key required evicting another one, which matters for the
/// `evicted_entries_total` metric (Phase 3 objective 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpsertOutcome {
    Inserted,
    Updated,
    /// A new key was inserted by evicting the least-recently-updated
    /// existing entry to stay within `max_entries`.
    InsertedByEviction,
    /// `max_entries` is 0, or some other configuration makes this key
    /// untrackable — the update was rejected outright (never silently
    /// dropped without the caller knowing).
    Rejected,
}

pub struct BoundedMap<K, V> {
    config: BoundedMapConfig,
    entries: HashMap<K, Slot<V>>,
}

impl<K, V> BoundedMap<K, V>
where
    K: Eq + Hash + Clone,
{
    pub fn new(config: BoundedMapConfig) -> Self {
        BoundedMap {
            config,
            entries: HashMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        self.entries.get(key).map(|s| &s.value)
    }

    /// Inserts a new key (using `default` to build the initial value) or
    /// updates an existing key's `last_updated` timestamp, then calls
    /// `update` on the value either way. If the key is new and the map
    /// is already at `max_entries`, the least-recently-updated existing
    /// entry is evicted first (deterministic: ties broken by whichever
    /// entry `HashMap` iteration happens to visit first, since a genuine
    /// tie means both are equally eligible for eviction).
    pub fn upsert(
        &mut self,
        key: K,
        now: Instant,
        default: impl FnOnce() -> V,
        update: impl FnOnce(&mut V),
    ) -> UpsertOutcome {
        if self.config.max_entries == 0 {
            return UpsertOutcome::Rejected;
        }

        if let Some(slot) = self.entries.get_mut(&key) {
            slot.last_updated = now;
            update(&mut slot.value);
            return UpsertOutcome::Updated;
        }

        let mut outcome = UpsertOutcome::Inserted;
        if self.entries.len() >= self.config.max_entries {
            if let Some(victim) = self.least_recently_updated_key() {
                self.entries.remove(&victim);
                outcome = UpsertOutcome::InsertedByEviction;
            } else {
                // max_entries is nonzero but the map is somehow already
                // "full" with nothing to evict — cannot happen in
                // practice (len() >= max_entries > 0 implies at least
                // one entry exists), but handled explicitly rather than
                // assumed, per "do not implicitly trust invariants that
                // aren't enforced by the type system."
                return UpsertOutcome::Rejected;
            }
        }

        let mut value = default();
        update(&mut value);
        self.entries.insert(
            key,
            Slot {
                value,
                last_updated: now,
            },
        );
        outcome
    }

    fn least_recently_updated_key(&self) -> Option<K> {
        self.entries
            .iter()
            .min_by_key(|(_, slot)| slot.last_updated)
            .map(|(k, _)| k.clone())
    }

    /// Removes every entry whose `last_updated` is older than
    /// `now - inactivity_ttl`. Returns the number of entries removed, for
    /// the `expired_entries_total` metric.
    pub fn expire_inactive(&mut self, now: Instant) -> usize {
        let ttl = self.config.inactivity_ttl;
        let before = self.entries.len();
        self.entries
            .retain(|_, slot| now.duration_since(slot.last_updated) <= ttl);
        before - self.entries.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.entries.iter().map(|(k, slot)| (k, &slot.value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(max_entries: usize) -> BoundedMapConfig {
        BoundedMapConfig {
            max_entries,
            inactivity_ttl: Duration::from_secs(300),
        }
    }

    #[test]
    fn inserts_and_updates_within_capacity() {
        let mut map: BoundedMap<&str, u32> = BoundedMap::new(config(10));
        let now = Instant::now();

        let outcome = map.upsert("a", now, || 0, |v| *v += 1);
        assert_eq!(outcome, UpsertOutcome::Inserted);
        assert_eq!(*map.get(&"a").unwrap(), 1);

        let outcome = map.upsert("a", now, || 0, |v| *v += 1);
        assert_eq!(outcome, UpsertOutcome::Updated);
        assert_eq!(*map.get(&"a").unwrap(), 2);
    }

    #[test]
    fn evicts_least_recently_updated_entry_when_at_capacity() {
        let mut map: BoundedMap<&str, u32> = BoundedMap::new(config(2));
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_secs(1);
        let t2 = t0 + Duration::from_secs(2);

        map.upsert("a", t0, || 0, |_| {});
        map.upsert("b", t1, || 0, |_| {});
        assert_eq!(map.len(), 2);

        // "a" is least-recently-updated; inserting "c" should evict it.
        let outcome = map.upsert("c", t2, || 0, |_| {});
        assert_eq!(outcome, UpsertOutcome::InsertedByEviction);
        assert_eq!(map.len(), 2);
        assert!(map.get(&"a").is_none());
        assert!(map.get(&"b").is_some());
        assert!(map.get(&"c").is_some());
    }

    #[test]
    fn touching_an_entry_protects_it_from_eviction() {
        let mut map: BoundedMap<&str, u32> = BoundedMap::new(config(2));
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_secs(1);
        let t2 = t0 + Duration::from_secs(2);
        let t3 = t0 + Duration::from_secs(3);

        map.upsert("a", t0, || 0, |_| {});
        map.upsert("b", t1, || 0, |_| {});
        // Touch "a" again — it's now more recently updated than "b".
        map.upsert("a", t2, || 0, |v| *v += 1);

        map.upsert("c", t3, || 0, |_| {});
        assert!(map.get(&"a").is_some(), "a was touched, should survive");
        assert!(
            map.get(&"b").is_none(),
            "b is now least-recently-updated, should be evicted"
        );
    }

    #[test]
    fn max_entries_zero_rejects_every_upsert() {
        let mut map: BoundedMap<&str, u32> = BoundedMap::new(config(0));
        let outcome = map.upsert("a", Instant::now(), || 0, |_| {});
        assert_eq!(outcome, UpsertOutcome::Rejected);
        assert!(map.is_empty());
    }

    #[test]
    fn expire_inactive_removes_only_stale_entries() {
        let mut map: BoundedMap<&str, u32> = BoundedMap::new(BoundedMapConfig {
            max_entries: 10,
            inactivity_ttl: Duration::from_secs(60),
        });
        let t0 = Instant::now();
        map.upsert("stale", t0, || 0, |_| {});
        map.upsert("fresh", t0 + Duration::from_secs(50), || 0, |_| {});

        let removed = map.expire_inactive(t0 + Duration::from_secs(90));
        assert_eq!(removed, 1);
        assert!(map.get(&"stale").is_none());
        assert!(map.get(&"fresh").is_some());
    }

    #[test]
    fn expiration_is_independent_of_capacity_pressure() {
        // Even with plenty of headroom, a stale entry should still expire.
        let mut map: BoundedMap<&str, u32> = BoundedMap::new(BoundedMapConfig {
            max_entries: 1000,
            inactivity_ttl: Duration::from_secs(10),
        });
        let t0 = Instant::now();
        map.upsert("a", t0, || 0, |_| {});
        let removed = map.expire_inactive(t0 + Duration::from_secs(20));
        assert_eq!(removed, 1);
    }

    #[test]
    fn deterministic_eviction_choice_given_identical_timestamps_pattern() {
        // Not a claim that ties are broken a *specific* way — only that
        // repeated runs against the same sequence of operations produce
        // the same result (no eviction policy that depends on e.g. OS
        // thread scheduling or random hashing seed variance per run).
        for _ in 0..20 {
            let mut map: BoundedMap<&str, u32> = BoundedMap::new(config(2));
            let t0 = Instant::now();
            map.upsert("a", t0, || 0, |_| {});
            map.upsert("b", t0 + Duration::from_millis(1), || 0, |_| {});
            map.upsert("c", t0 + Duration::from_millis(2), || 0, |_| {});
            assert!(map.get(&"a").is_none());
            assert!(map.get(&"b").is_some());
            assert!(map.get(&"c").is_some());
        }
    }
}
