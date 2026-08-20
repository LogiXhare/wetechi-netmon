//! Tenant-aware local-prefix registry and traffic direction
//! classification. See docs/architecture/direction-classification.md and
//! ADR 0002 (prefix lookup data structure).

mod direction;
mod registry;
mod trie;

pub use direction::{classify, ClassificationResult, Direction};
pub use registry::{
    build_registry, ConfigValidationError, OverlapDiagnostic, PrefixConfigEntry, PrefixEntry,
    PrefixMatch, PrefixRegistry, RegistryError, ValidationReport,
};

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;
    use std::net::{IpAddr, Ipv4Addr};

    proptest! {
        /// Longest-prefix-match must always pick the *most specific*
        /// registered prefix that covers the address, regardless of
        /// insertion order — this is the property the whole trie design
        /// (ADR 0002) exists to guarantee. A /24 derived from the same
        /// top 24 bits as `seed` always covers `seed` by construction,
        /// so it must always win over a /0 default, no matter which was
        /// inserted first.
        #[test]
        fn longest_prefix_always_wins_regardless_of_insertion_order(seed in any::<u32>()) {
            let specific_network = seed & 0xFFFF_FF00;

            let mut registry_default_first = PrefixRegistry::new();
            registry_default_first.insert(IpAddr::V4(Ipv4Addr::from(0u32)), 0, "default", None).unwrap();
            registry_default_first.insert(IpAddr::V4(Ipv4Addr::from(specific_network)), 24, "specific", None).unwrap();

            let mut registry_specific_first = PrefixRegistry::new();
            registry_specific_first.insert(IpAddr::V4(Ipv4Addr::from(specific_network)), 24, "specific", None).unwrap();
            registry_specific_first.insert(IpAddr::V4(Ipv4Addr::from(0u32)), 0, "default", None).unwrap();

            let lookup_addr = IpAddr::V4(Ipv4Addr::from(seed));
            let a = registry_default_first.lookup(lookup_addr).unwrap();
            let b = registry_specific_first.lookup(lookup_addr).unwrap();
            assert_eq!(a.matched_prefix_len, 24);
            assert_eq!(b.matched_prefix_len, 24);
        }

        /// The registry must never panic on arbitrary (even nonsensical)
        /// prefix lengths — invalid ones are rejected as errors, not a
        /// crash.
        #[test]
        fn insert_never_panics_on_arbitrary_prefix_length(len in any::<u8>()) {
            let mut registry = PrefixRegistry::new();
            let _ = registry.insert(IpAddr::V4(Ipv4Addr::new(10,0,0,0)), len, "t", None);
        }
    }
}
