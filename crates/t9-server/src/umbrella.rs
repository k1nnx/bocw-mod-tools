use anyhow::Result;
use t9_common::dw_protocol::ports;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tracing::{debug, info};

pub async fn run_umbrella_server() -> Result<()> {
    let addr = format!("127.0.0.1:{}", ports::UMBRELLA);
    let listener = TcpListener::bind(&addr).await?;
    info!("[umbrella] Umbrella server listening on {}", addr);

    loop {
        let (mut stream, peer) = listener.accept().await?;
        debug!("[umbrella] Connection from {}", peer);

        tokio::spawn(async move {
            let mut buf = vec![0u8; 8192];
            loop {
                let n = match stream.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(_) => break,
                };

                let request = String::from_utf8_lossy(&buf[..n]);
                debug!("[umbrella] Request: {}", &request[..request.len().min(200)]);

                // Accept everything, return 200 OK
                let response = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}";
                if stream.write_all(response.as_bytes()).await.is_err() {
                    break;
                }
            }
        });
    }
}
