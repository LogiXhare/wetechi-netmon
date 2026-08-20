//! A binary trie (PATRICIA-style, one bit per level) for deterministic
//! longest-prefix-match lookups. See
//! docs/architecture/decisions/0002-prefix-lookup-data-structure.md for
//! why this structure was chosen over a linear scan or a third-party
//! crate. Address bits are represented as `u128` regardless of family —
//! IPv4 addresses occupy the low 32 bits, IPv6 the full 128 — so one
//! implementation serves both; [`crate::registry::PrefixRegistry`] keeps
//! IPv4 and IPv6 in separate trie instances so the two address spaces
//! are never compared against each other.

#[derive(Debug, Default)]
struct Node<T> {
    children: [Option<Box<Node<T>>>; 2],
    entry: Option<T>,
}

impl<T> Node<T> {
    fn new() -> Self {
        Node {
            children: [None, None],
            entry: None,
        }
    }
}

/// One prefix already registered at or below/above the point being
/// inserted — used to report overlaps (see [`InsertOutcome`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlapKind {
    pub broader: bool, // true: existing prefix is broader (an ancestor); false: narrower (a descendant)
    pub existing_prefix_len: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InsertOutcome {
    pub overlaps: Vec<OverlapKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TrieError {
    #[error("prefix length {0} exceeds this trie's maximum of {1} bits")]
    PrefixTooLong(u8, u8),
    #[error("an entry for this exact prefix (same address bits and length) already exists")]
    DuplicatePrefix,
}

#[derive(Debug)]
pub struct PrefixTrie<T> {
    max_bits: u8,
    root: Node<T>,
}

impl<T> PrefixTrie<T> {
    pub fn new(max_bits: u8) -> Self {
        PrefixTrie {
            max_bits,
            root: Node::new(),
        }
    }

    /// Inserts `entry` for the prefix described by `addr_bits` (only the
    /// top `prefix_len` bits are significant) and `prefix_len`. Returns
    /// the set of already-registered prefixes this one overlaps with
    /// (broader ancestors and/or narrower descendants) — the caller
    /// decides what, if anything, to do about an overlap; only an exact
    /// duplicate is a hard error.
    pub fn insert(
        &mut self,
        addr_bits: u128,
        prefix_len: u8,
        entry: T,
    ) -> Result<InsertOutcome, TrieError> {
        if prefix_len > self.max_bits {
            return Err(TrieError::PrefixTooLong(prefix_len, self.max_bits));
        }

        let mut overlaps = Vec::new();
        let mut node = &mut self.root;
        for depth in 0..prefix_len {
            if node.entry.is_some() {
                overlaps.push(OverlapKind {
                    broader: true,
                    existing_prefix_len: depth, // the ancestor's own depth
                });
            }
            let bit = bit_at(addr_bits, depth, self.max_bits);
            node = node.children[bit as usize].get_or_insert_with(|| Box::new(Node::new()));
        }

        if node.entry.is_some() {
            return Err(TrieError::DuplicatePrefix);
        }

        // Any marked descendants below this point are narrower prefixes
        // already registered "inside" the one we're about to insert.
        collect_descendant_lengths(node, prefix_len, &mut overlaps);

        node.entry = Some(entry);
        Ok(InsertOutcome { overlaps })
    }

    /// Longest-prefix-match lookup. Returns the matching entry and the
    /// length of the prefix that matched, or `None` if nothing in the
    /// trie covers `addr_bits`.
    pub fn lookup(&self, addr_bits: u128) -> Option<(&T, u8)> {
        let mut node = &self.root;
        let mut best: Option<(&T, u8)> = None;

        if let Some(entry) = &node.entry {
            best = Some((entry, 0));
        }

        for depth in 0..self.max_bits {
            let bit = bit_at(addr_bits, depth, self.max_bits);
            match &node.children[bit as usize] {
                Some(child) => {
                    node = child;
                    if let Some(entry) = &node.entry {
                        best = Some((entry, depth + 1));
                    }
                }
                None => break,
            }
        }

        best
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.root.entry.is_none()
            && self.root.children[0].is_none()
            && self.root.children[1].is_none()
    }
}

fn bit_at(addr_bits: u128, depth: u8, max_bits: u8) -> u8 {
    let shift = max_bits - 1 - depth;
    ((addr_bits >> shift) & 1) as u8
}

fn collect_descendant_lengths<T>(node: &Node<T>, depth: u8, overlaps: &mut Vec<OverlapKind>) {
    for child in node.children.iter().flatten() {
        let child_depth = depth + 1;
        if child.entry.is_some() {
            overlaps.push(OverlapKind {
                broader: false,
                existing_prefix_len: child_depth,
            });
        }
        collect_descendant_lengths(child, child_depth, overlaps);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_trie_matches_nothing() {
        let trie: PrefixTrie<&str> = PrefixTrie::new(32);
        assert!(trie.is_empty());
        assert_eq!(trie.lookup(0x0A00_0001), None);
    }

    #[test]
    fn exact_match_and_longest_prefix_match() {
        let mut trie = PrefixTrie::new(32);
        // 10.0.0.0/8
        trie.insert(0x0A00_0000, 8, "site-wide").unwrap();
        // 10.0.0.0/24
        trie.insert(0x0A00_0000, 24, "specific-subnet").unwrap();

        // 10.0.0.5 should match the more specific /24, not the /8.
        let (entry, len) = trie.lookup(0x0A00_0005).unwrap();
        assert_eq!(*entry, "specific-subnet");
        assert_eq!(len, 24);

        // 10.5.0.5 only matches the /8.
        let (entry, len) = trie.lookup(0x0A05_0005).unwrap();
        assert_eq!(*entry, "site-wide");
        assert_eq!(len, 8);

        // 172.16.0.1 matches nothing.
        assert_eq!(trie.lookup(0xAC10_0001), None);
    }

    #[test]
    fn detects_exact_duplicate_as_an_error() {
        let mut trie = PrefixTrie::new(32);
        trie.insert(0x0A00_0000, 24, "first").unwrap();
        let result = trie.insert(0x0A00_0000, 24, "second");
        assert_eq!(result, Err(TrieError::DuplicatePrefix));
    }

    #[test]
    fn detects_broader_ancestor_overlap() {
        let mut trie = PrefixTrie::new(32);
        trie.insert(0x0A00_0000, 8, "broad").unwrap(); // 10.0.0.0/8
        let outcome = trie.insert(0x0A00_0000, 24, "narrow").unwrap(); // 10.0.0.0/24
        assert_eq!(outcome.overlaps.len(), 1);
        assert!(outcome.overlaps[0].broader);
        assert_eq!(outcome.overlaps[0].existing_prefix_len, 8);
    }

    #[test]
    fn detects_narrower_descendant_overlap() {
        let mut trie = PrefixTrie::new(32);
        trie.insert(0x0A00_0000, 24, "narrow").unwrap(); // 10.0.0.0/24
        let outcome = trie.insert(0x0A00_0000, 8, "broad").unwrap(); // 10.0.0.0/8, inserted second
        assert_eq!(outcome.overlaps.len(), 1);
        assert!(!outcome.overlaps[0].broader);
        assert_eq!(outcome.overlaps[0].existing_prefix_len, 24);
    }

    #[test]
    fn non_overlapping_prefixes_report_no_overlap() {
        let mut trie = PrefixTrie::new(32);
        trie.insert(0x0A00_0000, 24, "net-a").unwrap(); // 10.0.0.0/24
        let outcome = trie.insert(0x0A01_0000, 24, "net-b").unwrap(); // 10.1.0.0/24
        assert!(outcome.overlaps.is_empty());
    }

    #[test]
    fn rejects_prefix_length_exceeding_max_bits() {
        let mut trie: PrefixTrie<&str> = PrefixTrie::new(32);
        let result = trie.insert(0, 40, "invalid");
        assert_eq!(result, Err(TrieError::PrefixTooLong(40, 32)));
    }

    #[test]
    fn zero_length_prefix_matches_everything_as_a_fallback() {
        let mut trie = PrefixTrie::new(32);
        trie.insert(0, 0, "default").unwrap();
        assert_eq!(trie.lookup(0x0A00_0001), Some((&"default", 0)));
        assert_eq!(trie.lookup(0xFFFF_FFFF), Some((&"default", 0)));
    }

    #[test]
    fn ipv6_scale_lookup_128_bits() {
        let mut trie: PrefixTrie<&str> = PrefixTrie::new(128);
        // 2001:db8::/32
        let addr: u128 = 0x2001_0db8_0000_0000_0000_0000_0000_0000;
        trie.insert(addr, 32, "v6-net").unwrap();
        // 2001:db8:1234::1 (top 32 bits match)
        let lookup_addr: u128 = 0x2001_0db8_1234_0000_0000_0000_0000_0001;
        assert_eq!(trie.lookup(lookup_addr), Some((&"v6-net", 32)));
    }

    #[test]
    fn deterministic_repeated_lookups_are_identical() {
        let mut trie = PrefixTrie::new(32);
        trie.insert(0x0A00_0000, 8, "a").unwrap();
        trie.insert(0x0A00_0000, 16, "b").unwrap();
        let first = trie.lookup(0x0A00_1234);
        for _ in 0..1000 {
            assert_eq!(trie.lookup(0x0A00_1234), first);
        }
    }
}
