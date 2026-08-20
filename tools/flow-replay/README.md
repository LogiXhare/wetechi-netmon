# Flow Replay Tool

**Status:** Implemented (Phase 2 MVP scope).

Sends synthetic, well-formed IPFIX messages over UDP to a target
collector — for local/lab testing only. See
[../../docs/security-principles.md](../../docs/security-principles.md):
this tool must only ever be pointed at an authorized lab collector you
control, and only ever sends synthetic data (see `src/synthetic.rs`) —
never real captured traffic and never anything resembling attack traffic.

## Usage

```bash
cargo run -p wetechinetmon-flow-replay -- <target_host:port> [record_count]
# e.g. against a local collector:
cargo run -p wetechinetmon-flow-replay -- 127.0.0.1:2055 10
```

Sends one synthetic Template Set (template ID 256: sourceIPv4Address,
destinationIPv4Address, packetDeltaCount), then `record_count` (default 5)
synthetic Data Sets with incrementing sequence numbers.

## Testing

```bash
cargo test -p wetechinetmon-flow-replay
```

Includes a round-trip test that builds a synthetic message with this
tool's own byte-building code and decodes it with the real
`wetechinetmon-protocol-ipfix` decoder, confirming the values survive
unchanged — the same path a live exporter's traffic would take.
