//! `flow-replay` — sends synthetic IPFIX messages over UDP to a target
//! collector, for local/lab testing. See docs/security-principles.md:
//! this tool must only ever be pointed at authorized lab environments,
//! and only ever sends synthetic data (see `synthetic.rs`) — never real
//! captured traffic and never anything resembling attack traffic.

mod synthetic;

use tokio::net::UdpSocket;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let target = match args.next() {
        Some(t) => t,
        None => {
            eprintln!("usage: flow-replay <target_host:port> [record_count]");
            eprintln!(
                "  sends one synthetic Template Set, then <record_count> (default 5) synthetic Data Sets"
            );
            eprintln!("  ONLY point this at a lab/test collector you control — see docs/security-principles.md");
            std::process::exit(2);
        }
    };
    let record_count: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(5);

    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    socket.connect(&target).await?;

    let observation_domain_id = 1;
    let mut sequence_number = 1u32;

    let template = synthetic::template_message(sequence_number, observation_domain_id);
    socket.send(&template).await?;
    println!(
        "sent template message (template_id={})",
        synthetic::TEMPLATE_ID
    );
    sequence_number += 1;

    for i in 0..record_count {
        let src = [10, 0, 0, (i % 254 + 1) as u8];
        let dst = [10, 0, 1, ((i * 3) % 254 + 1) as u8];
        let packets = 100 + i as u64;
        let data =
            synthetic::data_message(sequence_number, observation_domain_id, src, dst, packets);
        socket.send(&data).await?;
        println!(
            "sent data record {}/{}: {:?} -> {:?}, packets={}",
            i + 1,
            record_count,
            src,
            dst,
            packets
        );
        sequence_number += 1;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    println!("done — sent 1 template message and {record_count} data messages to {target}");
    Ok(())
}
