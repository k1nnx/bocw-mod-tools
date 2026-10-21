//! Routing decisions for the Winsock hooks.
//!
//! Kept free of Windows APIs so the rules are unit-tested on every platform.
//! The DNS hooks hand out one marker loopback address per local service; the
//! `connect`/`sendto` hooks rewrite only those markers to the emulator's real
//! listener. Every other destination passes through untouched, and DemonWare
//! hosts without a local implementation are blocked instead of being sent to
//! an unrelated service.

use t9_common::dw_protocol::{hostnames, LocalService};

/// Loopback /24 used for marker addresses (127.84.9.0/24). Nothing listens
/// here, so a destination that misses the rewrite fails closed.
pub const MARKER_NET: [u8; 3] = [127, 84, 9];
pub const LOCALHOST: [u8; 4] = [127, 0, 0, 1];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostRoute {
    /// Resolve to this service's marker address.
    Redirect(LocalService),
    /// Fail the lookup.
    Block,
}

/// Decide what a DNS lookup for `hostname` should return. `None` means the
/// lookup is not ours and passes through to the real resolver.
pub fn route_host(hostname: &str) -> Option<HostRoute> {
    if hostnames::is_blocked_host(hostname) {
        return Some(HostRoute::Block);
    }
    if !hostnames::is_demonware_host(hostname) {
        return None;
    }

    let host = hostnames::normalize(hostname);
    let labels: Vec<&str> = host.split('.').collect();
    let has_label = |pred: &dyn Fn(&str) -> bool| labels.iter().any(|l| pred(l));

    let service = if has_label(&|l| {
        l == "loginservice" || l.ends_with("-loginservice") || l == "auth3" || l.ends_with("-auth3")
    }) {
        Some(LocalService::LoginService)
    } else if has_label(&|l| l == "stun" || l.ends_with("-stun")) {
        Some(LocalService::Stun)
    } else if has_label(&|l| l == "umbrella") {
        Some(LocalService::Umbrella)
    } else {
        // objectstore, exchange, ae, fileshare, ... have no local listener.
        None
    };

    Some(service.map_or(HostRoute::Block, HostRoute::Redirect))
}

pub fn marker_ip(service: LocalService) -> [u8; 4] {
    let index = LocalService::ALL
        .iter()
        .position(|s| *s == service)
        .expect("service is listed in LocalService::ALL");
    [MARKER_NET[0], MARKER_NET[1], MARKER_NET[2], index as u8 + 1]
}

/// NUL-terminated dotted marker address for passing to the real resolver.
pub fn marker_host_cstr(service: LocalService) -> Vec<u8> {
    let [a, b, c, d] = marker_ip(service);
    format!("{}.{}.{}.{}\0", a, b, c, d).into_bytes()
}

/// Map a marker destination to the emulator listener. Any other address,
/// including plain 127.0.0.1, is left alone.
pub fn rewrite_destination(ip: [u8; 4], _port: u16) -> Option<([u8; 4], u16)> {
    if ip[..3] != MARKER_NET {
        return None;
    }
    let service = LocalService::ALL.get((ip[3] as usize).checked_sub(1)?)?;
    Some((LOCALHOST, service.port()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn implemented_services_are_redirected() {
        for (host, service) in [
            ("loginservice.prod.demonware.net", LocalService::LoginService),
            ("navyblue-pc-loginservice.prod.demonware.net", LocalService::LoginService),
            ("navyblue-pc-auth3.prod.demonware.net", LocalService::LoginService),
            ("stun.us.demonware.net", LocalService::Stun),
            ("STUN.EU.DEMONWARE.NET.", LocalService::Stun),
            ("prod.umbrella.demonware.net", LocalService::Umbrella),
        ] {
            assert_eq!(route_host(host), Some(HostRoute::Redirect(service)), "{host}");
        }
    }

    #[test]
    fn unimplemented_demonware_hosts_are_blocked() {
        for host in [
            "objectstore.prod.demonware.net",
            "exchange.demonware.net",
            "ae.demonware.net",
            "fileshare.prod.demonware.net",
            "something-new.prod.demonware.net",
            "us.cdn.blizzard.com",
        ] {
            assert_eq!(route_host(host), Some(HostRoute::Block), "{host}");
        }
    }

    #[test]
    fn unrelated_hosts_pass_through() {
        for host in [
            "store.steampowered.com",
            "google.com",
            "demonware.net.example.com",
            "stun.l.google.com",
        ] {
            assert_eq!(route_host(host), None, "{host}");
        }
    }

    #[test]
    fn markers_round_trip_to_the_emulator_port() {
        for service in LocalService::ALL {
            let marker = marker_ip(service);
            for original_port in [443, 80, 3074, 0] {
                assert_eq!(
                    rewrite_destination(marker, original_port),
                    Some((LOCALHOST, service.port()))
                );
            }
            let text = String::from_utf8(marker_host_cstr(service)).unwrap();
            let [a, b, c, d] = marker;
            assert_eq!(text, format!("{a}.{b}.{c}.{d}\0"));
        }
    }

    #[test]
    fn only_markers_are_rewritten() {
        // The old hook redirected every external 80/443 connection and any
        // address on the emulator ports; neither may be rewritten now.
        assert_eq!(rewrite_destination([23, 45, 67, 89], 443), None);
        assert_eq!(rewrite_destination([23, 45, 67, 89], 3074), None);
        assert_eq!(rewrite_destination(LOCALHOST, 443), None);
        assert_eq!(rewrite_destination([127, 84, 9, 0], 443), None);
        assert_eq!(rewrite_destination([127, 84, 9, 200], 443), None);
    }
}
