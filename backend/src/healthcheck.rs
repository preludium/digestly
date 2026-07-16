//! Dependency-free container healthcheck (`digestly --healthcheck`).
//!
//! Used by the compose/Docker HEALTHCHECK so the runtime image needs no curl. Opens a raw
//! TCP connection to the local server, issues `GET /api/health`, and exits 0 only when the
//! status line is `200`. Kept out of the main HTTP client deps intentionally.

use std::time::Duration;

use anyhow::{bail, Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Probe the local health endpoint. Returns Ok(()) on HTTP 200, Err otherwise.
pub async fn run() -> Result<()> {
    // Connect to the local port; map a wildcard bind (0.0.0.0) to loopback.
    let bind = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    let port = bind.rsplit(':').next().unwrap_or("8080");
    let addr = format!("127.0.0.1:{port}");

    let fut = async {
        let mut stream = TcpStream::connect(&addr)
            .await
            .with_context(|| format!("connect {addr}"))?;
        let req = format!(
            "GET /api/health HTTP/1.0\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(req.as_bytes()).await.context("write request")?;

        let mut buf = Vec::with_capacity(1024);
        stream.read_to_end(&mut buf).await.context("read response")?;
        let head = String::from_utf8_lossy(&buf);
        let status_line = head.lines().next().unwrap_or("");
        if status_line.contains(" 200") {
            Ok(())
        } else {
            bail!("unhealthy: {status_line}");
        }
    };

    tokio::time::timeout(Duration::from_secs(5), fut)
        .await
        .context("healthcheck timed out")?
}
