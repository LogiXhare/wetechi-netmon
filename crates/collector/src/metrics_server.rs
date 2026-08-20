//! A minimal `/metrics` HTTP endpoint for Prometheus scraping.
//!
//! Deliberately hand-rolled instead of pulling in a full HTTP framework
//! (axum/hyper): the collector only needs to serve one static-ish
//! response body on one path, so a framework would be dependency weight
//! without a matching benefit at this phase. If the API/web crates need
//! a real HTTP framework later, that's evaluated on its own merits then
//! — see docs/dependency-license-matrix.md for the process.

use std::sync::Arc;

use prometheus::{Encoder, Registry, TextEncoder};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Serves `GET /metrics` (and a 404 for anything else) on `addr` until
/// the process exits. Runs forever — callers should `tokio::spawn` this.
pub async fn serve(addr: std::net::SocketAddr, registry: Arc<Registry>) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    tracing::info!(%addr, "metrics endpoint listening");

    loop {
        let (stream, peer) = listener.accept().await?;
        let registry = Arc::clone(&registry);
        tokio::spawn(async move {
            if let Err(err) = handle_connection(stream, &registry).await {
                tracing::debug!(%peer, error = %err, "metrics connection ended early");
            }
        });
    }
}

async fn handle_connection(
    mut stream: tokio::net::TcpStream,
    registry: &Registry,
) -> std::io::Result<()> {
    // We only need to know the request line to decide 200 vs. 404; the
    // body/headers of the request are irrelevant to us and are not read,
    // which is fine because we close the connection after responding
    // (no keep-alive to worry about).
    let mut buf = [0u8; 512];
    let n = stream.read(&mut buf).await?;
    let request_line = String::from_utf8_lossy(&buf[..n]);
    let is_metrics_path = request_line
        .split_whitespace()
        .nth(1)
        .map(|path| path == "/metrics")
        .unwrap_or(false);

    let response = if is_metrics_path {
        render_metrics_response(registry)
    } else {
        not_found_response()
    };

    stream.write_all(&response).await?;
    stream.shutdown().await?;
    Ok(())
}

fn render_metrics_response(registry: &Registry) -> Vec<u8> {
    let encoder = TextEncoder::new();
    let metric_families = registry.gather();
    let mut body = Vec::new();
    // Encoding failures here would mean a bug in how we registered
    // metrics (e.g. a name collision would have failed at registration
    // time already); if it somehow still fails, degrade to an empty body
    // with a 200 rather than taking the whole endpoint down.
    let _ = encoder.encode(&metric_families, &mut body);

    let mut response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        encoder.format_type(),
        body.len()
    )
    .into_bytes();
    response.extend_from_slice(&body);
    response
}

fn not_found_response() -> Vec<u8> {
    let body = b"not found\n";
    format!(
        "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes()
    .into_iter()
    .chain(body.iter().copied())
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::Metrics;
    use tokio::net::TcpStream;

    #[tokio::test]
    async fn serves_metrics_on_the_metrics_path() {
        let (metrics, registry) = Metrics::new().unwrap();
        metrics.datagrams_received_total.inc();
        let registry = Arc::new(registry);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            handle_connection(stream, &registry).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        client
            .write_all(b"GET /metrics HTTP/1.1\r\nHost: x\r\n\r\n")
            .await
            .unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).await.unwrap();

        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.contains("wetechinetmon_collector_flow_datagrams_received_total 1"));
    }

    #[tokio::test]
    async fn returns_404_for_other_paths() {
        let (_metrics, registry) = Metrics::new().unwrap();
        let registry = Arc::new(registry);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            handle_connection(stream, &registry).await.unwrap();
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        client
            .write_all(b"GET /health HTTP/1.1\r\nHost: x\r\n\r\n")
            .await
            .unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).await.unwrap();

        assert!(response.starts_with("HTTP/1.1 404"));
    }
}
