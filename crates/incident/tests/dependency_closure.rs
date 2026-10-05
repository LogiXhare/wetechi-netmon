//! Gate 2's structural safety property: nothing in the detection or
//! incident path can act on traffic or deliver a notification (ADR 0007,
//! ADR 0011, FU-9), proven from the dependency graph, not by review.
//!
//! - The detector's and the incident domain's normal-dependency closures
//!   must stay inside a reviewed allowlist. A new crate fails this test
//!   until someone reviews it and adds it here.
//! - No crate anywhere in the workspace may be a routing, device-access,
//!   packet, mail or chat crate.
//!
//! The graph comes from `cargo tree`, which reads `Cargo.lock`. Only the
//! host platform's dependencies appear, so the allowlist also holds the
//! Windows-only crates and the check is a subset, not an equality.

use std::collections::BTreeSet;
use std::process::Command;

/// Every crate the detector and incident domain may depend on, directly or
/// not. Reviewed 2026-10-05: logging, serialisation, errors and the
/// workspace's own pure crates. None does I/O beyond writing logs.
const ALLOWED: &[&str] = &[
    "cfg-if",
    "itoa",
    "lazy_static",
    "log",
    "matchers",
    "memchr",
    "nu-ansi-term",
    "once_cell",
    "pin-project-lite",
    "proc-macro2",
    "quote",
    "regex-automata",
    "regex-syntax",
    "serde",
    "serde_core",
    "serde_derive",
    "serde_json",
    "sharded-slab",
    "smallvec",
    "syn",
    "thiserror",
    "thiserror-impl",
    "thread_local",
    "tracing",
    "tracing-attributes",
    "tracing-core",
    "tracing-log",
    "tracing-serde",
    "tracing-subscriber",
    "unicode-ident",
    "wetechinetmon-aggregator",
    "wetechinetmon-classifier",
    "wetechinetmon-common",
    "wetechinetmon-detector",
    "wetechinetmon-incident",
    "windows-link",
    "windows-sys",
    "zmij",
];

/// Name fragments no crate in the workspace may carry in this release:
/// routing and device control (Phase 7 decides those), packet I/O, and
/// mail or chat delivery (Phase 6).
const FORBIDDEN: &[&str] = &[
    "bgp", "netconf", "snmp", "ssh", "telnet", "pnet", "netlink", "nftables", "iptables", "smtp",
    "lettre", "mail", "sendgrid", "twilio", "telegram", "teloxide", "slack", "discord", "serenity",
    "webhook",
];

/// The crate names in the normal-dependency closure that `args` selects.
fn closure(args: &[&str]) -> BTreeSet<String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(cargo)
        .args(["tree", "--quiet", "--edges", "normal", "--prefix", "none"])
        .args(["--format", "{p}"])
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo tree must run");
    assert!(
        output.status.success(),
        "cargo tree failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let names: BTreeSet<String> = String::from_utf8(output.stdout)
        .expect("cargo tree prints UTF-8")
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .map(str::to_string)
        .collect();
    assert!(!names.is_empty(), "cargo tree listed nothing");
    names
}

fn assert_allowed(package: &str) {
    let unreviewed: Vec<String> = closure(&["--package", package])
        .into_iter()
        .filter(|name| !ALLOWED.contains(&name.as_str()))
        .collect();
    assert!(
        unreviewed.is_empty(),
        "{package} now depends on {unreviewed:?}. Review each against ADR 0007 and ADR 0011 \
         (it must not act on traffic or deliver anything) before adding it to ALLOWED."
    );
}

#[test]
fn the_detector_depends_only_on_reviewed_crates() {
    assert_allowed("wetechinetmon-detector");
}

#[test]
fn the_incident_domain_depends_only_on_reviewed_crates() {
    assert_allowed("wetechinetmon-incident");
}

#[test]
fn no_crate_in_the_workspace_can_route_reach_a_device_or_deliver_a_message() {
    let workspace = closure(&["--workspace"]);
    let forbidden: Vec<&String> = workspace
        .iter()
        .filter(|name| FORBIDDEN.iter().any(|fragment| name.contains(fragment)))
        .collect();
    assert!(
        forbidden.is_empty(),
        "the workspace depends on {forbidden:?}; Phase 5 must not act on traffic or notify"
    );
    // The check is only meaningful if it saw the whole graph.
    assert!(workspace.contains("tokio-postgres") && workspace.contains("hyper"));
}
