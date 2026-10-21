/// DemonWare protocol constants and error types.

#[derive(Debug, thiserror::Error)]
pub enum DwError {
    #[error("buffer underflow: tried to read past end")]
    BufferUnderflow,
    #[error("type mismatch: expected {expected:#x}, got {actual:#x}")]
    TypeMismatch { expected: u8, actual: u8 },
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// DemonWare service IDs (from BO4 shield-development, applies to T9 lobby protocol).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ServiceId {
    BdStats = 4,
    BdProfiles = 8,
    BdTitleUtilities = 12,
    BdKeyArchive = 15,
    BdBandwidthTest = 18,
    BdCounter = 23,
    BdDml = 27,
    BdGroup = 28,
    BdAnticheat = 38,
    BdTags = 52,
    BdPooledStorage = 58,
    BdEventLog = 67,
    BdRichPresence = 68,
    BdMarketplace = 80,
    BdPublisherVariables = 95,
    BdMarketingComms = 104,
    BdUnk125 = 125,
    BdObjectStore = 193,
    BdLootGeneration = 195,
}

impl ServiceId {
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            4 => Some(Self::BdStats),
            8 => Some(Self::BdProfiles),
            12 => Some(Self::BdTitleUtilities),
            15 => Some(Self::BdKeyArchive),
            18 => Some(Self::BdBandwidthTest),
            23 => Some(Self::BdCounter),
            27 => Some(Self::BdDml),
            28 => Some(Self::BdGroup),
            38 => Some(Self::BdAnticheat),
            52 => Some(Self::BdTags),
            58 => Some(Self::BdPooledStorage),
            67 => Some(Self::BdEventLog),
            68 => Some(Self::BdRichPresence),
            80 => Some(Self::BdMarketplace),
            95 => Some(Self::BdPublisherVariables),
            104 => Some(Self::BdMarketingComms),
            125 => Some(Self::BdUnk125),
            193 => Some(Self::BdObjectStore),
            195 => Some(Self::BdLootGeneration),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::BdStats => "bdStats",
            Self::BdProfiles => "bdProfiles",
            Self::BdTitleUtilities => "bdTitleUtilities",
            Self::BdKeyArchive => "bdKeyArchive",
            Self::BdBandwidthTest => "bdBandwidthTest",
            Self::BdCounter => "bdCounter",
            Self::BdDml => "bdDML",
            Self::BdGroup => "bdGroup",
            Self::BdAnticheat => "bdAnticheat",
            Self::BdTags => "bdTags",
            Self::BdPooledStorage => "bdPooledStorage",
            Self::BdEventLog => "bdEventLog",
            Self::BdRichPresence => "bdRichPresence",
            Self::BdMarketplace => "bdMarketplace",
            Self::BdPublisherVariables => "bdPublisherVariables",
            Self::BdMarketingComms => "bdMarketingComms",
            Self::BdUnk125 => "bdUNK125",
            Self::BdObjectStore => "bdObjectStore",
            Self::BdLootGeneration => "bdLootGeneration",
        }
    }
}

/// T9 (BOCW) DemonWare server hostnames (verified from game binary analysis).
pub mod hostnames {
    pub const LOGIN_SERVICE: &str = "loginservice.prod.demonware.net";
    pub const LOGIN_SERVICE_ALT: &str = "navyblue-pc-loginservice.prod.demonware.net";
    pub const UMBRELLA: &str = "prod.umbrella.demonware.net";
    pub const OBJECT_STORE: &str = "objectstore.prod.demonware.net";
    pub const EXCHANGE: &str = "exchange.demonware.net";
    pub const AE: &str = "ae.demonware.net";

    pub const STUN_US: &str = "stun.us.demonware.net";
    pub const STUN_EU: &str = "stun.eu.demonware.net";
    pub const STUN_JP: &str = "stun.jp.demonware.net";
    pub const STUN_AU: &str = "stun.au.demonware.net";

    pub const BLOCKED_HOSTS: &[&str] = &[
        "us.cdn.blizzard.com",
        "us.actual.battle.net",
        "eu.cdn.blizzard.com",
        "eu.actual.battle.net",
        "level3.blizzard.com",
        "blzddist1-a.akamaihd.net",
        "level3.ssl.blizzard.com",
    ];

    /// True for `demonware.net`/`schild.net` and their subdomains. DNS names are
    /// case-insensitive and may carry a trailing dot.
    pub fn is_demonware_host(host: &str) -> bool {
        let host = normalize(host);
        ["demonware.net", "schild.net"]
            .iter()
            .any(|domain| host == *domain || host.ends_with(&format!(".{}", domain)))
    }

    pub fn is_blocked_host(host: &str) -> bool {
        let host = normalize(host);
        BLOCKED_HOSTS.iter().any(|h| host == *h)
    }

    pub fn normalize(host: &str) -> String {
        host.trim_end_matches('.').to_ascii_lowercase()
    }
}

/// Default ports for local emulator servers.
pub mod ports {
    pub const LOGIN_SERVICE: u16 = 3100;
    pub const LOBBY: u16 = 3200;
    pub const STUN: u16 = 3074;
    pub const UMBRELLA: u16 = 3300;
}

/// Game-local proxy DLL naming.
///
/// The game loads `XINPUT9_1_0.dll` from its own directory: a proxy installed
/// under this name was observed running in the game process (see
/// `docs/connection-diagnostics.md`). Windows ships no `XInput1_0.dll`, so the
/// earlier static-analysis note naming it is treated as a misread; that name is
/// only kept so diagnostics still report a stray copy from an older install.
pub mod proxy {
    pub const DLL_NAME: &str = "XINPUT9_1_0.dll";
    pub const BACKUP_NAME: &str = "XINPUT9_1_0.dll.bak";
    pub const LEGACY_DLL_NAMES: &[&str] = &["XInput1_0.dll"];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Tcp,
    Udp,
}

/// Services that `t9-server` actually listens for. The proxy only routes
/// traffic to these; anything else DemonWare-related is blocked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LocalService {
    LoginService,
    Lobby,
    Stun,
    Umbrella,
}

impl LocalService {
    pub const ALL: [LocalService; 4] = [
        LocalService::LoginService,
        LocalService::Lobby,
        LocalService::Stun,
        LocalService::Umbrella,
    ];

    pub fn port(self) -> u16 {
        match self {
            Self::LoginService => ports::LOGIN_SERVICE,
            Self::Lobby => ports::LOBBY,
            Self::Stun => ports::STUN,
            Self::Umbrella => ports::UMBRELLA,
        }
    }

    pub fn transport(self) -> Transport {
        match self {
            Self::Stun => Transport::Udp,
            _ => Transport::Tcp,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::LoginService => "Auth / Login Service",
            Self::Lobby => "Lobby Server",
            Self::Stun => "STUN Server",
            Self::Umbrella => "Umbrella",
        }
    }
}

/// Static session key used for the lobby handshake (same as shield-development).
pub const SESSION_KEY: &[u8; 24] = b"\x13\x37\x13\x37\x13\x37\x13\x37\x13\x37\x13\x37\x13\x37\x13\x37\x13\x37\x13\x37\x13\x37\x13\x37";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demonware_host_matching_is_suffix_based() {
        assert!(hostnames::is_demonware_host("loginservice.prod.demonware.net"));
        assert!(hostnames::is_demonware_host("STUN.US.DEMONWARE.NET."));
        assert!(!hostnames::is_demonware_host("demonware.net.example.com"));
        assert!(!hostnames::is_demonware_host("notdemonware.net"));
    }

    #[test]
    fn local_services_have_distinct_ports() {
        let mut ports: Vec<u16> = LocalService::ALL.iter().map(|s| s.port()).collect();
        ports.sort_unstable();
        ports.dedup();
        assert_eq!(ports.len(), LocalService::ALL.len());
        assert_eq!(LocalService::Stun.transport(), Transport::Udp);
    }
}
