use crate::config::ServerConfig;
use anyhow::Result;
use base64::Engine;
use serde_json::{json, Value};
use t9_common::dw_protocol::ports;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tracing::{debug, info};

/// Auth ticket structure matching DemonWare's format.
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct AuthTicket {
    magic_number: u32,
    ticket_type: u8,
    title_id: u32,
    time_issued: u32,
    time_expires: u32,
    license_id: u64,
    user_id: u64,
    username: [u8; 64],
    session_key: [u8; 24],
    using_hash_magic: [u8; 3],
    hash: [u8; 4],
}

pub async fn run_login_service(config: ServerConfig) -> Result<()> {
    let addr = format!("{}:{}", config.bind_addr, ports::LOGIN_SERVICE);
    let listener = TcpListener::bind(&addr).await?;
    info!("[auth] Login service listening on {}", addr);

    loop {
        let (mut stream, peer) = listener.accept().await?;
        let cfg = config.clone();
        debug!("[auth] Connection from {}", peer);

        tokio::spawn(async move {
            let mut buf = vec![0u8; 8192];
            loop {
                let n = match stream.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(_) => break,
                };

                let request = String::from_utf8_lossy(&buf[..n]);
                debug!("[auth] Request: {}", &request[..request.len().min(200)]);

                if request.starts_with("POST /auth/") || request.starts_with("GET /auth/") {
                    // Initial auth endpoint hit — game is starting auth flow
                    debug!("[auth] Auth endpoint hit, waiting for JSON body");
                    continue;
                }

                // Try to parse as JSON (the auth request body)
                let json_start = request.find('{');
                if let Some(start) = json_start {
                    let json_str = &request[start..];
                    match serde_json::from_str::<Value>(json_str) {
                        Ok(body) => {
                            let response = handle_auth_request(&cfg, &body);
                            let response_bytes = response.as_bytes();
                            if stream.write_all(response_bytes).await.is_err() {
                                break;
                            }
                            info!("[auth] Sent auth response to {}", peer);
                        }
                        Err(_) => {
                            debug!("[auth] Non-JSON data, treating as HTTP request");
                            let response = handle_http_request(&cfg, &request);
                            if stream.write_all(response.as_bytes()).await.is_err() {
                                break;
                            }
                        }
                    }
                } else {
                    let response = handle_http_request(&cfg, &request);
                    if stream.write_all(response.as_bytes()).await.is_err() {
                        break;
                    }
                }
            }
        });
    }
}

fn handle_http_request(config: &ServerConfig, request: &str) -> String {
    let b64 = base64::engine::general_purpose::STANDARD;

    // Handle REST login service endpoints
    if request.contains("/v1/login/") {
        info!("[auth] Login service REST request");
        let response_body = build_login_response(config);
        let body_str = serde_json::to_string(&response_body).unwrap_or_default();
        return format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body_str.len(),
            body_str
        );
    }

    if request.contains("/v1/") || request.contains("/v2/") {
        // Generic API endpoint — return empty success
        let body = json!({"status": "ok"});
        let body_str = body.to_string();
        return format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body_str.len(),
            body_str
        );
    }

    // Default 200 OK
    let _ = b64;
    "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".to_string()
}

fn handle_auth_request(config: &ServerConfig, body: &Value) -> String {
    let b64 = base64::engine::general_purpose::STANDARD;

    let title_id = body
        .get("title_id")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(config.title_id);

    let iv_seed = body
        .get("iv_seed")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);

    let identity = body
        .get("identity")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let session_key = t9_common::SESSION_KEY;

    // Build client ticket
    let mut ticket = AuthTicket {
        magic_number: 0xEFBDADDE,
        ticket_type: 0,
        title_id,
        time_issued: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as u32,
        time_expires: 0,
        license_id: 0,
        user_id: config.user_id,
        username: [0u8; 64],
        session_key: [0u8; 24],
        using_hash_magic: [0u8; 3],
        hash: [0u8; 4],
    };
    ticket.time_expires = ticket.time_issued + 30000;
    let name_bytes = config.username.as_bytes();
    let copy_len = name_bytes.len().min(63);
    ticket.username[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
    ticket.session_key.copy_from_slice(session_key);

    let ticket_bytes = unsafe {
        std::slice::from_raw_parts(
            &ticket as *const AuthTicket as *const u8,
            std::mem::size_of::<AuthTicket>(),
        )
    };
    let client_ticket_b64 = b64.encode(ticket_bytes);

    // Build server ticket (24 bytes session key + 104 bytes padding)
    let mut server_ticket = [0u8; 128];
    server_ticket[..24].copy_from_slice(session_key);
    let server_ticket_b64 = b64.encode(server_ticket);

    // Build extra_data
    let extra_data = json!({
        "username": config.username,
        "time_to_live": 9999,
        "extended_data": b64.encode("t9emu"),
    });

    // Build response JSON
    let response_json = json!({
        "auth_task": "85",
        "code": "700",
        "iv_seed": iv_seed.to_string(),
        "client_ticket": client_ticket_b64,
        "server_ticket": server_ticket_b64,
        "client_id": "treyarch-cod-t9-bnet",
        "account_type": "bnet",
        "crossplay_enabled": false,
        "loginqueue_enabled": false,
        "identity": identity,
        "extra_data": extra_data.to_string(),
        "service_level": "paid",
        "lsg_endpoint": null,
    });

    let body = serde_json::to_string(&response_json).unwrap();

    let now = chrono_lite_date();
    format!(
        "HTTP/1.1 200 OK\r\nServer: T9DemonWareEmulator/0.1\r\nContent-Type: application/json\r\nDate: {} GMT\r\nContent-Length: {}\r\n\r\n{}",
        now,
        body.len(),
        body
    )
}

fn build_login_response(config: &ServerConfig) -> Value {
    let b64 = base64::engine::general_purpose::STANDARD;
    let session_key = t9_common::SESSION_KEY;

    let mut ticket_data = AuthTicket {
        magic_number: 0xEFBDADDE,
        ticket_type: 0,
        title_id: config.title_id,
        time_issued: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as u32,
        time_expires: 0,
        license_id: 0,
        user_id: config.user_id,
        username: [0u8; 64],
        session_key: [0u8; 24],
        using_hash_magic: [0u8; 3],
        hash: [0u8; 4],
    };
    ticket_data.time_expires = ticket_data.time_issued + 30000;
    let name_bytes = config.username.as_bytes();
    let copy_len = name_bytes.len().min(63);
    ticket_data.username[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
    ticket_data.session_key.copy_from_slice(session_key);

    let ticket_bytes = unsafe {
        std::slice::from_raw_parts(
            &ticket_data as *const AuthTicket as *const u8,
            std::mem::size_of::<AuthTicket>(),
        )
    };

    let mut server_ticket = [0u8; 128];
    server_ticket[..24].copy_from_slice(session_key);

    json!({
        "title": {
            "titleID": config.title_id,
            "env": "prod",
        },
        "token": b64.encode(ticket_bytes),
        "serverTicket": b64.encode(server_ticket),
        "lsg": {
            "host": format!("{}:{}", config.bind_addr, t9_common::dw_protocol::ports::LOBBY),
        },
        "umbrella": {
            "token": b64.encode("t9emu_umbrella_token"),
            "expiry": 9999999,
        },
        "uno": {
            "unoID": config.user_id,
            "username": config.username,
        },
        "serviceLevel": "paid",
    })
}

fn chrono_lite_date() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    format!("timestamp-{}", now)
}
