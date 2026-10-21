use anyhow::Result;
use t9_common::dw_protocol::ports;
use t9_common::ByteBuffer;
use tokio::net::UdpSocket;
use tracing::{debug, info, warn};

pub async fn run_stun_server() -> Result<()> {
    let addr = format!("127.0.0.1:{}", ports::STUN);
    let socket = UdpSocket::bind(&addr).await?;
    info!("[stun] STUN server listening on {}", addr);

    let mut buf = [0u8; 1024];
    loop {
        let (n, peer) = socket.recv_from(&mut buf).await?;
        if n < 3 {
            continue;
        }

        let packet_type = buf[0];
        let _version = buf[1];
        let _padding = buf[2];

        match packet_type {
            30 => {
                // IP discovery
                debug!("[stun] IP discovery from {}", peer);
                let response = build_ip_discovery_response(peer.ip());
                socket.send_to(&response, peer).await?;
            }
            20 => {
                // NAT discovery
                debug!("[stun] NAT discovery from {}", peer);
                let response = build_nat_discovery_response(peer.ip());
                socket.send_to(&response, peer).await?;
            }
            other => {
                warn!("[stun] Unknown packet type {} from {}", other, peer);
            }
        }
    }
}

fn build_ip_discovery_response(peer_ip: std::net::IpAddr) -> Vec<u8> {
    let ip = match peer_ip {
        std::net::IpAddr::V4(v4) => u32::from_le_bytes(v4.octets()),
        _ => 0x0100007f, // 127.0.0.1 fallback
    };

    let mut buf = ByteBuffer::new();
    buf.set_use_data_types(false);
    buf.write_ubyte(31); // response type
    buf.write_ubyte(2); // version
    buf.write_ubyte(0); // padding
    buf.write_uint32(ip); // external IP
    buf.write_uint16(ports::STUN); // port
    buf.into_buffer()
}

fn build_nat_discovery_response(peer_ip: std::net::IpAddr) -> Vec<u8> {
    let ip = match peer_ip {
        std::net::IpAddr::V4(v4) => u32::from_le_bytes(v4.octets()),
        _ => 0x0100007f,
    };

    let mut buf = ByteBuffer::new();
    buf.set_use_data_types(false);
    buf.write_ubyte(21); // response type
    buf.write_ubyte(2); // version
    buf.write_ubyte(0); // padding
    buf.write_uint32(ip); // external IP
    buf.write_uint16(ports::STUN); // port
    buf.write_uint32(0x0100007f); // server IP (127.0.0.1)
    buf.write_uint16(ports::STUN); // server port
    buf.into_buffer()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn discovery_responses_echo_the_peer_address() {
        let peer = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

        let ip = build_ip_discovery_response(peer);
        assert_eq!(&ip[..3], &[31, 2, 0]);
        assert_eq!(&ip[3..7], &[127, 0, 0, 1]);
        assert_eq!(u16::from_le_bytes([ip[7], ip[8]]), ports::STUN);

        let nat = build_nat_discovery_response(peer);
        assert_eq!(&nat[..3], &[21, 2, 0]);
        assert_eq!(&nat[3..7], &[127, 0, 0, 1]);
        assert_eq!(nat.len(), 15);
    }
}
