use crate::config::ServerConfig;
use crate::crypto::LobbyCrypto;
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
    crypto: LobbyCrypto,
    service_registry: ServiceRegistry,
}

impl LobbyConnection {
    fn new() -> Self {
        Self {
            authenticated: false,
            crypto: LobbyCrypto::new(),
            service_registry: ServiceRegistry::new(),
        }
    }
}

pub async fn run_lobby_server(config: ServerConfig) -> Result<()> {
    let addr = format!("{}:{}", config.bind_addr, ports::LOBBY);
    let listener = TcpListener::bind(&addr).await?;
    info!("[lobby] Lobby server listening on {}", addr);

    loop {
        let (mut stream, peer) = listener.accept().await?;
        debug!("[lobby] Connection from {}", peer);

        tokio::spawn(async move {
            let mut conn = LobbyConnection::new();

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
            conn.crypto.queue_packet_to_hash(&remaining);

            // Send server_header_ack
            // msgType=0x81, serverSelectedProto=0xD2, cypher210ConnID + serverNonce
            let server_ack: Vec<u8> = vec![
                0x16, 0x00, 0x00, 0x00, // size = 22
                0xAB, 0x81, // magic + type
                0xD2, 0x00, 0x00, 0x00, // protocol 210
                0x13, 0x37, 0x13, 0x37, 0x13, 0x37, 0x13, 0x37, // connID
                0x13, 0x37, 0x13, 0x37, 0x13, 0x37, 0x13, 0x37, // nonce
            ];
            conn.crypto.queue_packet_to_hash(&server_ack);

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
                conn.crypto.queue_packet_to_hash(&auth_packet);
                conn.crypto.derive_keys();

                conn.authenticated = true;

                // Send server_auth_done (type 0x83)
                let response_id = conn.crypto.response_id();
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
                match conn.crypto.decrypt_lobby_packet(encrypted, &seed) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::build_success_reply;

    fn client_header_ack() -> Vec<u8> {
        let mut packet = vec![0xC8, 0x00, 0x00, 0x00, 0xC8, 0x00, 0x00, 0x00];
        packet.extend_from_slice(&[0x42; 16]); // client nonce
        packet
    }

    fn client_auth() -> Vec<u8> {
        let mut packet = vec![0x2A, 0x00, 0x00, 0x00, 0xAB, 0x82];
        packet.extend_from_slice(&[0x55; 32]); // auth body
        packet.extend_from_slice(&[0x99; 8]); // trailing hash
        packet
    }

    fn service_call(conn: &LobbyConnection, service_id: u8, task_id: u8) -> Vec<u8> {
        let mut plain = ByteBuffer::new();
        plain.set_use_data_types(false);
        plain.write_uint32(3);
        plain.write_ubyte(0x86);
        plain.write_ubyte(service_id);
        plain.write_ubyte(task_id);
        let seed = [0x24u8; 16];
        let encrypted = conn
            .crypto
            .encrypt_lobby_packet(&plain.into_buffer(), &seed)
            .unwrap();

        let mut body = vec![0xAB, 0x85];
        body.extend_from_slice(&1u32.to_le_bytes()); // message count
        body.extend_from_slice(&seed);
        body.extend_from_slice(&encrypted);
        body.extend_from_slice(&[0u8; 8]); // hash
        let mut packet = (body.len() as i32).to_le_bytes().to_vec();
        packet.extend_from_slice(&body);
        packet
    }

    fn authenticated_connection() -> LobbyConnection {
        let mut conn = LobbyConnection::new();
        handle_lobby_packet(&mut conn, &client_header_ack()).unwrap();
        handle_lobby_packet(&mut conn, &client_auth()).unwrap();
        conn
    }

    #[test]
    fn header_ack_gets_server_header_ack() {
        let mut conn = LobbyConnection::new();
        let responses = handle_lobby_packet(&mut conn, &client_header_ack()).unwrap();
        assert_eq!(responses.len(), 1);
        let ack = &responses[0];
        assert_eq!(i32::from_le_bytes(ack[..4].try_into().unwrap()) as usize, ack.len() - 4);
        assert_eq!(&ack[4..6], &[0xAB, 0x81]);
        assert_eq!(u32::from_le_bytes(ack[6..10].try_into().unwrap()), 210);
        assert!(!conn.authenticated);
    }

    #[test]
    fn client_auth_completes_handshake() {
        let mut conn = LobbyConnection::new();
        handle_lobby_packet(&mut conn, &client_header_ack()).unwrap();
        let responses = handle_lobby_packet(&mut conn, &client_auth()).unwrap();
        assert!(conn.authenticated);
        let mut expected = vec![0x0A, 0x00, 0x00, 0x00, 0xAB, 0x83];
        expected.extend_from_slice(&t9_common::SESSION_KEY[16..24]);
        assert_eq!(responses, vec![expected]);
    }

    #[test]
    fn service_call_before_auth_is_ignored() {
        let authed = authenticated_connection();
        let packet = service_call(&authed, 38, 2);
        let mut fresh = LobbyConnection::new();
        assert!(handle_lobby_packet(&mut fresh, &packet).unwrap().is_empty());
    }

    #[test]
    fn encrypted_service_call_is_dispatched() {
        let mut conn = authenticated_connection();
        for (service_id, task_id) in [(4u8, 1u8), (38, 2), (250, 7)] {
            let packet = service_call(&conn, service_id, task_id);
            let responses = handle_lobby_packet(&mut conn, &packet).unwrap();
            assert_eq!(responses, vec![build_success_reply(task_id)], "service {service_id}");
        }
    }

    #[test]
    fn connections_do_not_share_crypto_state() {
        let _authed = authenticated_connection();
        let fresh = LobbyConnection::new();
        assert!(fresh.crypto.decrypt_lobby_packet(&[0; 16], &[0; 16]).is_err());
    }

    #[test]
    fn zero_size_packet_gets_zero_reply() {
        let mut conn = LobbyConnection::new();
        let responses = handle_lobby_packet(&mut conn, &[0, 0, 0, 0]).unwrap();
        assert_eq!(responses, vec![vec![0, 0, 0, 0]]);
    }
}
