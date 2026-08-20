//! Tenant-aware local-prefix registry: IPv4 + IPv6, longest-prefix
//! match, duplicate/overlap detection, and hostgroup membership.
//! FR-3.1/FR-3.2.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::trie::{InsertOutcome, PrefixTrie, TrieError};

/// One registered local prefix's metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefixEntry {
    pub tenant: String,
    pub hostgroup: Option<String>,
    pub prefix_len: u8,
}

/// The result of a successful lookup: which entry matched, and how
/// specific the match was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrefixMatch<'a> {
    pub entry: &'a PrefixEntry,
    pub matched_prefix_len: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    #[error("prefix length {0} exceeds this address family's maximum of {1} bits")]
    PrefixTooLong(u8, u8),
    #[error("duplicate prefix: an entry for this exact network/length already exists")]
    DuplicatePrefix,
}

impl From<TrieError> for RegistryError {
    fn from(e: TrieError) -> Self {
        match e {
            TrieError::PrefixTooLong(got, max) => RegistryError::PrefixTooLong(got, max),
            TrieError::DuplicatePrefix => RegistryError::DuplicatePrefix,
        }
    }
}

/// A human-readable overlap diagnostic — never fatal on its own (unlike
/// an exact duplicate), but worth surfacing to whoever is configuring
/// prefixes (FR-3.2 "overlapping-prefix diagnostics").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlapDiagnostic {
    pub network: IpAddr,
    pub prefix_len: u8,
    pub message: String,
}

#[derive(Debug)]
pub struct PrefixRegistry {
    ipv4: PrefixTrie<PrefixEntry>,
    ipv6: PrefixTrie<PrefixEntry>,
    len: usize,
}

impl Default for PrefixRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PrefixRegistry {
    pub fn new() -> Self {
        PrefixRegistry {
            ipv4: PrefixTrie::new(32),
            ipv6: PrefixTrie::new(128),
            len: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn len(&self) -> usize {
        self.len
    }

    /// Inserts one prefix. Returns overlap diagnostics on success (may be
    /// empty); returns an error only for an invalid prefix length or an
    /// exact duplicate.
    pub fn insert(
        &mut self,
        network: IpAddr,
        prefix_len: u8,
        tenant: impl Into<String>,
        hostgroup: Option<String>,
    ) -> Result<Vec<OverlapDiagnostic>, RegistryError> {
        let entry = PrefixEntry {
            tenant: tenant.into(),
            hostgroup,
            prefix_len,
        };

        let outcome: InsertOutcome = match network {
            IpAddr::V4(addr) => self
                .ipv4
                .insert(ipv4_bits(addr), prefix_len, entry)
                .map_err(RegistryError::from)?,
            IpAddr::V6(addr) => self
                .ipv6
                .insert(ipv6_bits(addr), prefix_len, entry)
                .map_err(RegistryError::from)?,
        };

        self.len += 1;

        Ok(outcome
            .overlaps
            .into_iter()
            .map(|o| OverlapDiagnostic {
                network,
                prefix_len,
                message: if o.broader {
                    format!(
                        "{network}/{prefix_len} is contained within an already-registered /{}",
                        o.existing_prefix_len
                    )
                } else {
                    format!(
                        "{network}/{prefix_len} already contains a more specific, already-registered /{}",
                        o.existing_prefix_len
                    )
                },
            })
            .collect())
    }

    /// Longest-prefix-match lookup, deterministic for a given registry
    /// state and address.
    pub fn lookup(&self, addr: IpAddr) -> Option<PrefixMatch<'_>> {
        match addr {
            IpAddr::V4(a) => {
                let (entry, len) = self.ipv4.lookup(ipv4_bits(a))?;
                Some(PrefixMatch {
                    entry,
                    matched_prefix_len: len,
                })
            }
            IpAddr::V6(a) => {
                let (entry, len) = self.ipv6.lookup(ipv6_bits(a))?;
                Some(PrefixMatch {
                    entry,
                    matched_prefix_len: len,
                })
            }
        }
    }
}

fn ipv4_bits(addr: Ipv4Addr) -> u128 {
    u32::from(addr) as u128
}

fn ipv6_bits(addr: Ipv6Addr) -> u128 {
    u128::from(addr)
}

/// One raw configuration entry, as an operator would write it, before
/// validation. See docs/configuration/prefixes.md.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefixConfigEntry {
    pub network: IpAddr,
    pub prefix_len: u8,
    pub tenant: String,
    pub hostgroup: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigValidationError {
    #[error("entry #{index} ({network}/{prefix_len}): {source}")]
    InvalidEntry {
        index: usize,
        network: IpAddr,
        prefix_len: u8,
        source: RegistryError,
    },
}

#[derive(Debug)]
pub struct ValidationReport {
    pub registry: PrefixRegistry,
    pub warnings: Vec<OverlapDiagnostic>,
}

/// Builds a [`PrefixRegistry`] from raw configuration entries, collecting
/// **every** validation error rather than stopping at the first one — an
/// operator fixing a prefix-list typo wants to see all the problems in
/// one pass, not one-at-a-time.
pub fn build_registry(
    entries: &[PrefixConfigEntry],
) -> Result<ValidationReport, Vec<ConfigValidationError>> {
    let mut registry = PrefixRegistry::new();
    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    for (index, entry) in entries.iter().enumerate() {
        match registry.insert(
            entry.network,
            entry.prefix_len,
            entry.tenant.clone(),
            entry.hostgroup.clone(),
        ) {
            Ok(mut overlaps) => warnings.append(&mut overlaps),
            Err(source) => errors.push(ConfigValidationError::InvalidEntry {
                index,
                network: entry.network,
                prefix_len: entry.prefix_len,
                source,
            }),
        }
    }

    if errors.is_empty() {
        Ok(ValidationReport { registry, warnings })
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn ipv4_and_ipv6_are_kept_in_separate_address_spaces() {
        let mut registry = PrefixRegistry::new();
        registry
            .insert(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 0)), 8, "tenant-a", None)
            .unwrap();

        // An IPv6 address that happens to share low-order bits with the
        // IPv4 prefix must not match — they're different tries entirely.
        assert!(registry
            .lookup(IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0x0a00, 0)))
            .is_none());
        assert!(registry
            .lookup(IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3)))
            .is_some());
    }

    #[test]
    fn tenant_and_hostgroup_are_returned_on_match() {
        let mut registry = PrefixRegistry::new();
        registry
            .insert(
                IpAddr::V4(Ipv4Addr::new(10, 0, 0, 0)),
                24,
                "tenant-a",
                Some("edge-hosts".to_string()),
            )
            .unwrap();

        let m = registry
            .lookup(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)))
            .unwrap();
        assert_eq!(m.entry.tenant, "tenant-a");
        assert_eq!(m.entry.hostgroup.as_deref(), Some("edge-hosts"));
        assert_eq!(m.matched_prefix_len, 24);
    }

    #[test]
    fn duplicate_prefix_is_rejected() {
        let mut registry = PrefixRegistry::new();
        registry
            .insert(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 0)), 24, "a", None)
            .unwrap();
        let result = registry.insert(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 0)), 24, "b", None);
        assert_eq!(result, Err(RegistryError::DuplicatePrefix));
    }

    #[test]
    fn overlap_is_reported_but_not_rejected() {
        let mut registry = PrefixRegistry::new();
        registry
            .insert(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 0)), 8, "a", None)
            .unwrap();
        let warnings = registry
            .insert(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 0)), 24, "b", None)
            .unwrap();
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn build_registry_collects_all_errors_not_just_the_first() {
        let entries = vec![
            PrefixConfigEntry {
                network: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 0)),
                prefix_len: 24,
                tenant: "a".into(),
                hostgroup: None,
            },
            PrefixConfigEntry {
                network: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 0)),
                prefix_len: 24, // duplicate of the first
                tenant: "b".into(),
                hostgroup: None,
            },
            PrefixConfigEntry {
                network: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 0)),
                prefix_len: 99, // invalid length
                tenant: "c".into(),
                hostgroup: None,
            },
        ];

        let result = build_registry(&entries);
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 2);
    }

    #[test]
    fn build_registry_succeeds_and_reports_overlap_warnings() {
        let entries = vec![
            PrefixConfigEntry {
                network: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 0)),
                prefix_len: 8,
                tenant: "a".into(),
                hostgroup: None,
            },
            PrefixConfigEntry {
                network: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 0)),
                prefix_len: 24,
                tenant: "a".into(),
                hostgroup: Some("subnet".into()),
            },
        ];

        let report = build_registry(&entries).unwrap();
        assert_eq!(report.registry.len(), 2);
        assert_eq!(report.warnings.len(), 1);
    }

    #[test]
    fn lookup_on_empty_registry_matches_nothing() {
        let registry = PrefixRegistry::new();
        assert!(registry.is_empty());
        assert!(registry
            .lookup(IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4)))
            .is_none());
    }
}
