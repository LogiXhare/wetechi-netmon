# Telemetry Collector

**Status:** Implemented (Phase 2 MVP scope — IPFIX only; NetFlow/sFlow are
later phases).

Binds a UDP socket, decodes IPFIX messages via `wetechinetmon-protocol-ipfix`,
tracks one template cache per exporter (with restart detection via
sequence-number regression), and exposes Prometheus metrics on a separate
HTTP port.

## Running

```bash
# Optional — defaults shown:
export WETECHINETMON_COLLECTOR_BIND=0.0.0.0:2055
export WETECHINETMON_COLLECTOR_METRICS_BIND=0.0.0.0:9090
export RUST_LOG=info

cargo run --bin wetechinetmon-collector
# metrics: curl http://localhost:9090/metrics
```

Send it synthetic test traffic with the replay tool:
[../../tools/flow-replay](../../tools/flow-replay). **Never point this
collector at a network capture of real traffic or point `flow-replay` at
anything but a lab collector you control** — see
[../../docs/security-principles.md](../../docs/security-principles.md).

## Metrics

See `src/metrics.rs` for the full list (all prefixed
`wetechinetmon_collector_`). Full configuration-option documentation
(defaults, security implications, verification commands) lives in
[../../docs/configuration/index.md](../../docs/configuration/index.md).

## Known limitations

- Configuration is environment-variable-only for now — the real
  Configuration Service (`crates/configuration`) doesn't exist yet; this
  is a documented MVP simplification, not an oversight (see
  `src/config.rs`).
- Graceful shutdown handles Ctrl+C but not `SIGTERM` — acceptable for
  Phase 2 manual/dev use; systemd integration (which sends `SIGTERM`) is
  a Phase 9 deliverable and will need this revisited then.
- `udp_receive_buffer_errors_total` (listed in the master prompt's metric
  set) is not implemented — reading the kernel's UDP drop counter
  portably across Windows/Linux needs platform-specific code this phase
  doesn't need yet.
- No fuzz testing at the collector layer itself yet (the underlying
  parser has property-based tests — see
  [../protocol-ipfix/README.md](../protocol-ipfix/README.md)).

## Testing

```bash
cargo test -p wetechinetmon-collector
```

16 unit tests, including a full malformed-header, sequence-number-restart,
unknown-template, and metrics-server integration-style test (real
`tokio::net::TcpListener`/`TcpStream`, not mocked).
