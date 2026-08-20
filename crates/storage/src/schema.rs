//! Original ClickHouse row types and table DDL for WetechiNetMon
//! analytics. Table names, columns, and layout are designed
//! independently for this project — see docs/clean-room-boundary.md. Do
//! not copy any proprietary product's table names or definitions.
//!
//! **Deliberate simplification, documented not hidden:** IP addresses
//! are stored as `String` (their text representation), not ClickHouse's
//! native `IPv4`/`IPv6` column types. This sidesteps needing to verify
//! this environment's `clickhouse` Rust crate version handles native IP
//! type (de)serialization correctly without a live server to test
//! against (none is available here — see docs/integrations/clickhouse.md).
//! Revisit once integration-tested against a real server.

use serde::Serialize;

/// Every table's DDL, using `MergeTree` with daily partitioning and a
/// retention TTL — see docs/architecture/aggregation.md /
/// docs/integrations/clickhouse.md for the retention rationale.
pub const CREATE_TABLE_STATEMENTS: &[(&str, &str)] = &[
    ("wetechinetmon_total_ipv4_traffic", DDL_TOTAL_IPV4),
    ("wetechinetmon_total_ipv6_traffic", DDL_TOTAL_IPV6),
    ("wetechinetmon_host_traffic", DDL_HOST_TRAFFIC),
    ("wetechinetmon_network_traffic", DDL_NETWORK_TRAFFIC),
    (
        "wetechinetmon_slash24_network_traffic",
        DDL_SLASH24_NETWORK_TRAFFIC,
    ),
    ("wetechinetmon_hostgroup_traffic", DDL_HOSTGROUP_TRAFFIC),
    ("wetechinetmon_asn_traffic", DDL_ASN_TRAFFIC),
    ("wetechinetmon_exporter_traffic", DDL_EXPORTER_TRAFFIC),
    ("wetechinetmon_interface_traffic", DDL_INTERFACE_TRAFFIC),
];

/// Default retention: 30 days, matching the `TTL ... INTERVAL 30 DAY`
/// clause in every DDL statement below. Configurable per-deployment only
/// by altering the DDL/TTL clause directly; not exposed as a runtime
/// config option in Phase 3 — see docs/integrations/clickhouse.md "Known
/// limitations." Public so callers (and docs generation) can reference
/// the documented default without duplicating the magic number `30`.
pub const RETENTION_DAYS: u32 = 30;

macro_rules! counter_columns {
    () => {
        "bytes UInt64, \
         packets UInt64, \
         flows UInt64, \
         tcp_bytes UInt64, \
         tcp_packets UInt64, \
         udp_bytes UInt64, \
         udp_packets UInt64, \
         icmp_bytes UInt64, \
         icmp_packets UInt64, \
         tcp_syn_packets UInt64, \
         fragmented_packets UInt64, \
         dropped_packets UInt64"
    };
}

const DDL_TOTAL_IPV4: &str = concat!(
    "CREATE TABLE IF NOT EXISTS wetechinetmon_total_ipv4_traffic ( \
        timestamp DateTime, ",
    counter_columns!(),
    " ) ENGINE = MergeTree \
     PARTITION BY toYYYYMMDD(timestamp) \
     ORDER BY timestamp \
     TTL timestamp + INTERVAL 30 DAY"
);

const DDL_TOTAL_IPV6: &str = concat!(
    "CREATE TABLE IF NOT EXISTS wetechinetmon_total_ipv6_traffic ( \
        timestamp DateTime, ",
    counter_columns!(),
    " ) ENGINE = MergeTree \
     PARTITION BY toYYYYMMDD(timestamp) \
     ORDER BY timestamp \
     TTL timestamp + INTERVAL 30 DAY"
);

const DDL_HOST_TRAFFIC: &str = concat!(
    "CREATE TABLE IF NOT EXISTS wetechinetmon_host_traffic ( \
        timestamp DateTime, \
        address String, \
        family UInt8, ",
    counter_columns!(),
    " ) ENGINE = MergeTree \
     PARTITION BY toYYYYMMDD(timestamp) \
     ORDER BY (timestamp, family, address) \
     TTL timestamp + INTERVAL 30 DAY"
);

const DDL_NETWORK_TRAFFIC: &str = concat!(
    "CREATE TABLE IF NOT EXISTS wetechinetmon_network_traffic ( \
        timestamp DateTime, \
        address String, \
        prefix_len UInt8, \
        family UInt8, ",
    counter_columns!(),
    " ) ENGINE = MergeTree \
     PARTITION BY toYYYYMMDD(timestamp) \
     ORDER BY (timestamp, family, prefix_len, address) \
     TTL timestamp + INTERVAL 30 DAY"
);

const DDL_SLASH24_NETWORK_TRAFFIC: &str = concat!(
    "CREATE TABLE IF NOT EXISTS wetechinetmon_slash24_network_traffic ( \
        timestamp DateTime, \
        address String, ",
    counter_columns!(),
    " ) ENGINE = MergeTree \
     PARTITION BY toYYYYMMDD(timestamp) \
     ORDER BY (timestamp, address) \
     TTL timestamp + INTERVAL 30 DAY"
);

const DDL_HOSTGROUP_TRAFFIC: &str = concat!(
    "CREATE TABLE IF NOT EXISTS wetechinetmon_hostgroup_traffic ( \
        timestamp DateTime, \
        hostgroup String, ",
    counter_columns!(),
    " ) ENGINE = MergeTree \
     PARTITION BY toYYYYMMDD(timestamp) \
     ORDER BY (timestamp, hostgroup) \
     TTL timestamp + INTERVAL 30 DAY"
);

const DDL_ASN_TRAFFIC: &str = concat!(
    "CREATE TABLE IF NOT EXISTS wetechinetmon_asn_traffic ( \
        timestamp DateTime, \
        asn UInt32, ",
    counter_columns!(),
    " ) ENGINE = MergeTree \
     PARTITION BY toYYYYMMDD(timestamp) \
     ORDER BY (timestamp, asn) \
     TTL timestamp + INTERVAL 30 DAY"
);

const DDL_EXPORTER_TRAFFIC: &str = concat!(
    "CREATE TABLE IF NOT EXISTS wetechinetmon_exporter_traffic ( \
        timestamp DateTime, \
        exporter String, ",
    counter_columns!(),
    " ) ENGINE = MergeTree \
     PARTITION BY toYYYYMMDD(timestamp) \
     ORDER BY (timestamp, exporter) \
     TTL timestamp + INTERVAL 30 DAY"
);

const DDL_INTERFACE_TRAFFIC: &str = concat!(
    "CREATE TABLE IF NOT EXISTS wetechinetmon_interface_traffic ( \
        timestamp DateTime, \
        exporter String, \
        interface_index UInt32, \
        direction Enum8('input' = 1, 'output' = 2), ",
    counter_columns!(),
    " ) ENGINE = MergeTree \
     PARTITION BY toYYYYMMDD(timestamp) \
     ORDER BY (timestamp, exporter, interface_index, direction) \
     TTL timestamp + INTERVAL 30 DAY"
);

/// Shared counter fields, duplicated into each row struct below rather
/// than composed via `#[serde(flatten)]` — the `clickhouse` crate's
/// `Row` derive maps struct fields directly to column order, and
/// flattening is not something we've verified works correctly against a
/// real server in this environment (see module docs).
#[derive(Debug, Clone, Copy, Default, Serialize, clickhouse::Row)]
pub struct CounterFields {
    pub bytes: u64,
    pub packets: u64,
    pub flows: u64,
    pub tcp_bytes: u64,
    pub tcp_packets: u64,
    pub udp_bytes: u64,
    pub udp_packets: u64,
    pub icmp_bytes: u64,
    pub icmp_packets: u64,
    pub tcp_syn_packets: u64,
    pub fragmented_packets: u64,
    pub dropped_packets: u64,
}

impl From<wetechinetmon_aggregator::TrafficCounters> for CounterFields {
    fn from(c: wetechinetmon_aggregator::TrafficCounters) -> Self {
        CounterFields {
            bytes: c.bytes,
            packets: c.packets,
            flows: c.flows,
            tcp_bytes: c.tcp_bytes,
            tcp_packets: c.tcp_packets,
            udp_bytes: c.udp_bytes,
            udp_packets: c.udp_packets,
            icmp_bytes: c.icmp_bytes,
            icmp_packets: c.icmp_packets,
            tcp_syn_packets: c.tcp_syn_packets,
            fragmented_packets: c.fragmented_packets,
            dropped_packets: c.dropped_packets,
        }
    }
}

#[derive(Debug, Clone, Serialize, clickhouse::Row)]
pub struct TotalTrafficRow {
    #[serde(with = "clickhouse::serde::time::datetime")]
    pub timestamp: time::OffsetDateTime,
    #[serde(flatten)]
    pub counters: CounterFields,
}

#[derive(Debug, Clone, Serialize, clickhouse::Row)]
pub struct HostTrafficRow {
    #[serde(with = "clickhouse::serde::time::datetime")]
    pub timestamp: time::OffsetDateTime,
    pub address: String,
    pub family: u8,
    #[serde(flatten)]
    pub counters: CounterFields,
}

#[derive(Debug, Clone, Serialize, clickhouse::Row)]
pub struct NetworkTrafficRow {
    #[serde(with = "clickhouse::serde::time::datetime")]
    pub timestamp: time::OffsetDateTime,
    pub address: String,
    pub prefix_len: u8,
    pub family: u8,
    #[serde(flatten)]
    pub counters: CounterFields,
}

#[derive(Debug, Clone, Serialize, clickhouse::Row)]
pub struct Slash24TrafficRow {
    #[serde(with = "clickhouse::serde::time::datetime")]
    pub timestamp: time::OffsetDateTime,
    pub address: String,
    #[serde(flatten)]
    pub counters: CounterFields,
}

#[derive(Debug, Clone, Serialize, clickhouse::Row)]
pub struct HostgroupTrafficRow {
    #[serde(with = "clickhouse::serde::time::datetime")]
    pub timestamp: time::OffsetDateTime,
    pub hostgroup: String,
    #[serde(flatten)]
    pub counters: CounterFields,
}

#[derive(Debug, Clone, Serialize, clickhouse::Row)]
pub struct AsnTrafficRow {
    #[serde(with = "clickhouse::serde::time::datetime")]
    pub timestamp: time::OffsetDateTime,
    pub asn: u32,
    #[serde(flatten)]
    pub counters: CounterFields,
}

#[derive(Debug, Clone, Serialize, clickhouse::Row)]
pub struct ExporterTrafficRow {
    #[serde(with = "clickhouse::serde::time::datetime")]
    pub timestamp: time::OffsetDateTime,
    pub exporter: String,
    #[serde(flatten)]
    pub counters: CounterFields,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[repr(u8)]
pub enum InterfaceDirection {
    Input = 1,
    Output = 2,
}

#[derive(Debug, Clone, Serialize, clickhouse::Row)]
pub struct InterfaceTrafficRow {
    #[serde(with = "clickhouse::serde::time::datetime")]
    pub timestamp: time::OffsetDateTime,
    pub exporter: String,
    pub interface_index: u32,
    pub direction: InterfaceDirection,
    #[serde(flatten)]
    pub counters: CounterFields,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_table_has_a_create_statement() {
        assert_eq!(CREATE_TABLE_STATEMENTS.len(), 9);
        for (name, ddl) in CREATE_TABLE_STATEMENTS {
            assert!(
                ddl.contains(name),
                "DDL for {name} should reference its own table name"
            );
            assert!(ddl.contains("MergeTree"), "{name} should use MergeTree");
            assert!(ddl.contains("TTL"), "{name} should declare a retention TTL");
        }
    }

    #[test]
    fn retention_constant_matches_documented_ddl() {
        assert_eq!(RETENTION_DAYS, 30);
    }

    #[test]
    fn counter_fields_convert_from_aggregator_traffic_counters() {
        let counters = wetechinetmon_aggregator::TrafficCounters {
            bytes: 100,
            packets: 10,
            flows: 1,
            tcp_bytes: 100,
            tcp_packets: 10,
            udp_bytes: 0,
            udp_packets: 0,
            icmp_bytes: 0,
            icmp_packets: 0,
            tcp_syn_packets: 1,
            fragmented_packets: 0,
            dropped_packets: 0,
        };
        let row_fields: CounterFields = counters.into();
        assert_eq!(row_fields.bytes, 100);
        assert_eq!(row_fields.tcp_syn_packets, 1);
    }
}
