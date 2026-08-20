//! ClickHouse integration test — exercises the real write path
//! (migrations + batch write) against an actual ClickHouse server.
//!
//! **Skips cleanly, not silently, when no server is configured.** Set
//! `CLICKHOUSE_TEST_URL` (e.g. `http://localhost:8123`) to run this for
//! real. Per `prompts/CLAUDE_MASTER_PROMPT.md` §30 rule 10 ("do not
//! claim tests passed unless they were actually executed"), this prints
//! a clear skip message and exits successfully rather than either (a)
//! failing every run in environments without ClickHouse, or (b)
//! pretending to have verified something it didn't.
//!
//! No Docker/ClickHouse server was available in the environment this was
//! developed in — this test has not been executed for real here. See
//! docs/integrations/clickhouse.md.

use std::time::{Duration, Instant};

use time::OffsetDateTime;
use wetechinetmon_storage::schema::{CounterFields, TotalTrafficRow};
use wetechinetmon_storage::{run_migrations, BatchConfig, ClickHouseWriter, Client, RetryConfig};

#[tokio::test]
async fn migrations_and_batch_write_round_trip_against_a_real_server() {
    let Ok(url) = std::env::var("CLICKHOUSE_TEST_URL") else {
        eprintln!(
            "SKIPPED: CLICKHOUSE_TEST_URL not set — this test needs a real ClickHouse server. \
             See docs/integrations/clickhouse.md. Not run in this environment (no Docker/server available)."
        );
        return;
    };

    let client = Client::default().with_url(&url);

    run_migrations(&client)
        .await
        .expect("migrations should apply against a real server");

    let now = Instant::now();
    let mut writer = ClickHouseWriter::new(
        client.clone(),
        "wetechinetmon_total_ipv4_traffic",
        BatchConfig {
            max_rows: 1,
            max_interval: Duration::from_millis(1),
        },
        RetryConfig::default(),
        now,
    );

    writer.push(TotalTrafficRow {
        timestamp: OffsetDateTime::now_utc(),
        counters: CounterFields {
            bytes: 1234,
            packets: 12,
            flows: 1,
            ..Default::default()
        },
    });

    let report = writer.tick(now + Duration::from_millis(10)).await;
    assert_eq!(
        report.write_failures, 0,
        "write should succeed against a live server"
    );
    assert_eq!(report.rows_written, 1);
}
