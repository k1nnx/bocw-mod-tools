use crate::config::ServerConfig;
use crate::crypto;
use crate::services::ServiceRegistry;
use anyhow::Result;
use t9_common::dw_protocol::ports;
use t9_common::ByteBuffer;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tracing::{debug, info, warn};

/// Lobby server state per connection.
struct LobbyConnection {
    authenticated: bool,
    service_registry: ServiceRegistry,
}

pub async fn run_lobby_server(config: ServerConfig) -> Result<()> {
    let addr = format!("{}:{}", config.bind_addr, ports::LOBBY);
    let listener = TcpListener::bind(&addr).await?;
    info!("[lobby] Lobby server listening on {}", addr);

    loop {
        let (mut stream, peer) = listener.accept().await?;
        debug!("[lobby] Connection from {}", peer);

        tokio::spawn(async move {
            let mut conn = LobbyConnection {
                authenticated: false,
                service_registry: ServiceRegistry::new(),
            };

            let mut buf = vec![0u8; 65536];
            loop {
                let n = match stream.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(e) => {
                        debug!("[lobby] Read error from {}: {}", peer, e);
                        break;
                    }
                };

                let packet = &buf[..n];
                match handle_lobby_packet(&mut conn, packet) {
                    Ok(responses) => {
                        for response in responses {
                            if stream.write_all(&response).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        warn!("[lobby] Error handling packet from {}: {}", peer, e);
                    }
                }
            }
            debug!("[lobby] Connection closed: {}", peer);
        });
    }
}

fn handle_lobby_packet(
    conn: &mut LobbyConnection,
    packet: &[u8],
) -> Result<Vec<Vec<u8>>> {
    let mut responses = Vec::new();
    let mut buffer = ByteBuffer::from_bytes(packet.to_vec());
    buffer.set_use_data_types(false);

    while buffer.has_more_data() {
        let size = buffer.read_int32()?;

        if size <= 0 {
            // Zero-size packet — respond with zero
            responses.push(vec![0, 0, 0, 0]);
            return Ok(responses);
        }

        if size == 0xC8 {
            // Client header ack — phase 1 of handshake
            debug!("[lobby] Received client_header_ack (0xC8)");

            let _c8 = buffer.read_int32()?;
            let remaining = buffer.get_remaining().to_vec();
            crypto::queue_packet_to_hash(&remaining);

            // Send server_header_ack
            // msgType=0x81, serverSelectedProto=0xD2, cypher210ConnID + serverNonce
            let server_ack: Vec<u8> = vec![
                0x16, 0x00, 0x00, 0x00, // size = 22
                0xAB, 0x81, // magic + type
                0xD2, 0x00, 0x00, 0x00, // protocol 210
                0x13, 0x37, 0x13, 0x37, 0x13, 0x37, 0x13, 0x37, // connID
                0x13, 0x37, 0x13, 0x37, 0x13, 0x37, 0x13, 0x37, // nonce
            ];
            crypto::queue_packet_to_hash(&server_ack);

            debug!("[lobby] Sending server_header_ack");
            responses.push(server_ack);
            return Ok(responses);
        }

        // Check for 0xAB magic byte
        let check_ab = buffer.read_ubyte()?;
        if check_ab != 0xAB {
            warn!("[lobby] Expected 0xAB, got {:#x}", check_ab);
            return Ok(responses);
        }

        let msg_type = buffer.read_ubyte()?;

        match msg_type {
            0x82 => {
                // Client auth — phase 2 of handshake
                debug!("[lobby] Received client_auth (0x82)");

                let auth_packet = packet[..packet.len().saturating_sub(8)].to_vec();
                crypto::queue_packet_to_hash(&auth_packet);
                crypto::derive_keys();

                conn.authenticated = true;

                // Send server_auth_done (type 0x83)
                let response_id = crypto::get_response_id();
                let mut reply = vec![0x0A, 0x00, 0x00, 0x00, 0xAB, 0x83];
                reply.extend_from_slice(&response_id);

                debug!("[lobby] Sending server_auth_done");
                responses.push(reply);
                return Ok(responses);
            }
            0x85 => {
                // Encrypted service call — phase 3
                if !conn.authenticated {
                    warn!("[lobby] Service call before authentication!");
                    return Ok(responses);
                }

                let _msg_count = buffer.read_uint32()?;
                let seed = buffer.read_bytes(16)?;
                let remaining = buffer.get_remaining();

                if remaining.len() < 8 {
                    warn!("[lobby] Encrypted payload too short");
                    return Ok(responses);
                }

                let encrypted = &remaining[..remaining.len() - 8];
                let _hash = &remaining[remaining.len() - 8..];

                // Decrypt the service call
                match crypto::decrypt_lobby_packet(encrypted, &seed) {
                    Ok(decrypted) => {
                        let mut serv = ByteBuffer::from_bytes(decrypted);
                        serv.set_use_data_types(false);

                        let _serv_size = serv.read_uint32()?;
                        let _magic = serv.read_ubyte()?; // 0x86
                        let service_id = serv.read_ubyte()?;

                        let service_data = serv.get_remaining().to_vec();
                        let reply_data =
                            conn.service_registry.dispatch(service_id, &service_data);
                        responses.push(reply_data);
                    }
                    Err(e) => {
                        warn!("[lobby] Decryption failed: {}", e);
                        // Send empty success reply to keep the game happy
                        let reply = build_empty_reply(0);
                        responses.push(reply);
                    }
                }

                return Ok(responses);
            }
            other => {
                warn!("[lobby] Unknown message type: {:#x}", other);
                return Ok(responses);
            }
        }
    }

    Ok(responses)
}

fn build_empty_reply(task_id: u8) -> Vec<u8> {
    let mut reply = ByteBuffer::new();
    reply.set_use_data_types(false);

    // Build a minimal success reply
    let mut inner = ByteBuffer::new();
    inner.set_use_data_types(false);
    inner.write_uint32(0); // transaction ID
    inner.write_ubyte(task_id);
    inner.write_ubyte(0); // success status

    let inner_data = inner.into_buffer();
    reply.write_int32(inner_data.len() as i32);
    reply.write_raw(&inner_data);

    reply.into_buffer()
}
