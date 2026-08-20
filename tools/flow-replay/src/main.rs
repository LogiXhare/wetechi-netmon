//! `flow-replay` — sends synthetic IPFIX messages over UDP to a target
//! collector, for local/lab testing. See docs/security-principles.md:
//! this tool must only ever be pointed at authorized lab environments,
//! and only ever sends synthetic data (see `synthetic.rs`) — never real
//! captured traffic and never anything resembling attack traffic.
//!
//! Address convention (Phase 3, objective 11 "incoming/outgoing/
//! internal/other"): this tool doesn't know the target collector's
//! configured local prefixes, so it uses a fixed, documented convention
//! — `10.0.0.0/8` / `2001:db8::/32` as "local", `203.0.113.0/24`
//! (RFC 5737 TEST-NET-3) / `2606:4700::/32` as "external" — and expects
//! the collector under test to have `10.0.0.0/8`/`2001:db8::/32`
//! configured as a local prefix (see
//! docs/development/flow-replay.md).

mod synthetic;

use std::net::{Ipv4Addr, Ipv6Addr};
use synthetic::IpProtocol;
use tokio::net::UdpSocket;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scenario {
    Incoming,
    Outgoing,
    Internal,
    Other,
}

impl Scenario {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "incoming" => Some(Scenario::Incoming),
            "outgoing" => Some(Scenario::Outgoing),
            "internal" => Some(Scenario::Internal),
            "other" => Some(Scenario::Other),
            _ => None,
        }
    }

    /// (source, destination) IPv4 addresses for this scenario, per the
    /// module-level address convention.
    fn addrs_v4(self, i: u8) -> (Ipv4Addr, Ipv4Addr) {
        let local = Ipv4Addr::new(10, 0, 0, i % 254 + 1);
        let external_a = Ipv4Addr::new(203, 0, 113, i % 254 + 1);
        let external_b = Ipv4Addr::new(198, 51, 100, i % 254 + 1);
        match self {
            Scenario::Incoming => (external_a, local),
            Scenario::Outgoing => (local, external_a),
            Scenario::Internal => (local, Ipv4Addr::new(10, 0, 1, i % 254 + 1)),
            Scenario::Other => (external_a, external_b),
        }
    }

    fn addrs_v6(self, i: u8) -> (Ipv6Addr, Ipv6Addr) {
        let local = Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, i as u16 + 1);
        let external_a = Ipv6Addr::new(0x2606, 0x4700, 0, 0, 0, 0, 0, i as u16 + 1);
        let external_b = Ipv6Addr::new(0x2620, 0x00fe, 0, 0, 0, 0, 0, i as u16 + 1);
        match self {
            Scenario::Incoming => (external_a, local),
            Scenario::Outgoing => (local, external_a),
            Scenario::Internal => (
                local,
                Ipv6Addr::new(0x2001, 0xdb8, 0, 1, 0, 0, 0, i as u16 + 1),
            ),
            Scenario::Other => (external_a, external_b),
        }
    }
}

fn parse_protocol(s: &str) -> Option<IpProtocol> {
    match s {
        "tcp" => Some(IpProtocol::Tcp),
        "udp" => Some(IpProtocol::Udp),
        "icmp" => Some(IpProtocol::Icmp),
        _ => None,
    }
}

fn print_usage() {
    eprintln!("usage: flow-replay <target_host:port> [options]");
    eprintln!();
    eprintln!("options:");
    eprintln!("  --count N          number of data records to send (default 5)");
    eprintln!("  --scenario S       incoming | outgoing | internal | other (default incoming)");
    eprintln!("  --family F         ipv4 | ipv6 (default ipv4)");
    eprintln!("  --protocol P       tcp | udp | icmp (default tcp)");
    eprintln!(
        "  --exporters N      number of distinct observation domains to simulate (default 1)"
    );
    eprintln!(
        "  --sampling-rate N  advertise this sampling rate via an Options Template (default: none)"
    );
    eprintln!();
    eprintln!(
        "ONLY point this at a lab/test collector you control — see docs/security-principles.md"
    );
    eprintln!("Address convention: see docs/development/flow-replay.md");
}

struct Args {
    target: String,
    count: u32,
    scenario: Scenario,
    ipv6: bool,
    protocol: IpProtocol,
    exporters: u32,
    sampling_rate: Option<u32>,
}

fn parse_args() -> Option<Args> {
    let mut args = std::env::args().skip(1);
    let target = args.next()?;
    let mut count = 5u32;
    let mut scenario = Scenario::Incoming;
    let mut ipv6 = false;
    let mut protocol = IpProtocol::Tcp;
    let mut exporters = 1u32;
    let mut sampling_rate = None;

    while let Some(flag) = args.next() {
        let value = args.next()?;
        match flag.as_str() {
            "--count" => count = value.parse().ok()?,
            "--scenario" => scenario = Scenario::parse(&value)?,
            "--family" => ipv6 = value == "ipv6",
            "--protocol" => protocol = parse_protocol(&value)?,
            "--exporters" => exporters = value.parse().ok()?,
            "--sampling-rate" => sampling_rate = Some(value.parse().ok()?),
            _ => return None,
        }
    }

    Some(Args {
        target,
        count,
        scenario,
        ipv6,
        protocol,
        exporters,
        sampling_rate,
    })
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let Some(args) = parse_args() else {
        print_usage();
        std::process::exit(2);
    };

    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    socket.connect(&args.target).await?;

    for exporter in 0..args.exporters {
        let observation_domain_id = exporter + 1;
        let mut sequence_number = 1u32;

        if let Some(rate) = args.sampling_rate {
            let opt_tmpl =
                synthetic::options_template_message(sequence_number, observation_domain_id);
            socket.send(&opt_tmpl).await?;
            sequence_number += 1;
            let opt_data =
                synthetic::options_data_message(sequence_number, observation_domain_id, 1, rate);
            socket.send(&opt_data).await?;
            sequence_number += 1;
            println!("[exporter {exporter}] sent sampling options (rate={rate}) for observation domain {observation_domain_id}");
        }

        let template = if args.ipv6 {
            synthetic::template_message_ipv6(sequence_number, observation_domain_id)
        } else {
            synthetic::template_message_ipv4(sequence_number, observation_domain_id)
        };
        socket.send(&template).await?;
        sequence_number += 1;
        println!("[exporter {exporter}] sent template message (observation_domain_id={observation_domain_id})");

        for i in 0..args.count {
            let packets = 10 + i as u64;
            let bytes = 1000 + i as u64 * 100;
            let data = if args.ipv6 {
                let (src, dst) = args.scenario.addrs_v6(i as u8);
                synthetic::data_message_ipv6(
                    sequence_number,
                    observation_domain_id,
                    src,
                    dst,
                    51000,
                    443,
                    args.protocol,
                    bytes,
                    packets,
                )
            } else {
                let (src, dst) = args.scenario.addrs_v4(i as u8);
                synthetic::data_message_ipv4(
                    sequence_number,
                    observation_domain_id,
                    src,
                    dst,
                    51000,
                    443,
                    args.protocol,
                    bytes,
                    packets,
                )
            };
            socket.send(&data).await?;
            println!(
                "[exporter {exporter}] sent data record {}/{} ({:?}, {:?})",
                i + 1,
                args.count,
                args.scenario,
                args.protocol
            );
            sequence_number += 1;
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    println!(
        "done — sent traffic from {} exporter(s) to {}",
        args.exporters, args.target
    );
    Ok(())
}
