# BOCW (T9) DemonWare Emulator — Architecture Design

## Overview

A Rust-based DemonWare emulator for Call of Duty: Black Ops Cold War (T9), enabling
the game to run fully offline or on custom/private servers without connecting to
Activision's infrastructure. Based on analysis of project-bo4/shield-development (BO4/T8).

**Safety model**: Never connects to official Activision servers. All DemonWare traffic
is intercepted and handled locally. No process injection, no DLL injection into the
game process at runtime — we use a proxy DLL approach (static file swap).

---

## How Shield (BO4) Works — Reference Architecture

### Proxy DLL Injection
- Ships as `XInput9_1_0.dll` — a DLL the game loads automatically
- On load, patches the game's IAT (Import Address Table) to redirect Winsock calls
- Hooks: `getaddrinfo`, `freeaddrinfo`, `gethostbyname`, `connect`, `closesocket`,
  `send`, `recv`, `sendto`, `recvfrom`, `select`, `ioctlsocket`, `getpeername`, `getsockname`

### Server Components
| Server | Protocol | Hostname (BO4) | Purpose |
|--------|----------|-----------------|---------|
| auth3_server | TCP (HTTP/JSON) | ops4-pc-auth3.prod.demonware.net | Authentication tickets |
| lobby_server | TCP (binary, encrypted) | ops4-pc-lobby.prod.demonware.net | Game services (20+ sub-services) |
| stun_server | UDP (binary) | ops4-stun.{us,eu,jp,au}.demonware.net | NAT discovery, IP reflection |
| umbrella_server | TCP (HTTP) | prod.umbrella.demonware.net | Telemetry/analytics (stub) |
| fileshare_server | TCP | ops4-fileshare.prod.schild.net | User-generated content |

### DemonWare Services (Lobby Server sub-services)
| ID | Service | Priority |
|----|---------|----------|
| 4 | bdStats | Unknown |
| 8 | bdProfiles | Used |
| 12 | bdTitleUtilities | **Essential** |
| 15 | bdKeyArchive | Low |
| 18 | bdBandwidthTest | Used |
| 23 | bdCounter | Used |
| 27 | bdDML | **Essential** |
| 28 | bdGroup | Low |
| 38 | bdAnticheat | **Critical** (must respond to keep game happy) |
| 52 | bdTags | Used |
| 58 | bdPooledStorage | Used |
| 67 | bdEventLog | Used |
| 68 | bdRichPresence | Used |
| 80 | bdMarketplace | Used |
| 95 | bdPublisherVariables | Used |
| 104 | bdMarketingComms | Used |
| 125 | bdUNK125 | Patched out in BO4 |
| 193 | bdObjectStore | **Essential** |
| 195 | bdLootGeneration | Unknown |

### Anti-Tamper (Arxan) Bypass
- Disables TLS callbacks (prevent early integrity checks)
- Hooks `CreateThread` to restore TLS after Arxan's own init
- Hooks `GetThreadContext` to hide debug registers from game code
- Hooks `CreateMutexExA` to hide debugger mutexes (IDA)
- Hooks `NtQuerySystemInformation` + `NtQueryInformationProcess` to scrub
  "evil keywords" (IDA, x64dbg, Cheat Engine, Wireshark, HxD) from process listings
- Patches ~1219 integrity check locations using signature scanning
- Hooks `GetWindowTextA` to scrub window titles

### Platform Bypasses (BO4-specific, will differ for T9)
- Patches `BattleNet_IsDisabled` → returns true
- Patches `BattleNet_IsConnected` → returns true
- Patches `bdAuth::validateResponseSignature` → always succeeds
- Patches `bdAuthPC::processPlatformData` → always succeeds
- Detoures `bnet_get_username` and `bnet_get_userid` to local config values
- Patches cross-auth, QoS, and contract checks

---

## Our T9 Architecture (Rust)

### Approach: Two-Binary Design

```
┌─────────────────────────────────────────────────┐
│                   BOCW Game                       │
│  (loads our proxy DLL instead of real one)        │
│                                                   │
│  Winsock calls ──→ Our hooks ──→ Local servers    │
└───────────────────────┬─────────────────────────┘
                        │ IPC / localhost
┌───────────────────────┴─────────────────────────┐
│              t9-server (Rust binary)              │
│                                                   │
│  ┌──────────┐ ┌──────────┐ ┌──────────────────┐  │
│  │ Auth3    │ │ Lobby    │ │ STUN (UDP)       │  │
│  │ (HTTP)   │ │ (binary) │ │ IP discovery     │  │
│  └──────────┘ └──────────┘ └──────────────────┘  │
│  ┌──────────┐ ┌──────────┐                        │
│  │ Umbrella │ │ Fileshare│                        │
│  │ (HTTP)   │ │ (TCP)    │                        │
│  └──────────┘ └──────────┘                        │
└─────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────┐
│       t9-proxy.dll (Rust cdylib)                  │
│                                                   │
│  - Loaded as proxy DLL by game                    │
│  - Hooks Winsock via IAT patching                 │
│  - Redirects DW hostnames to 127.0.0.1            │
│  - Blocks CDN/Blizzard/Activision hosts           │
│  - Patches integrity checks (Arxan equivalent)    │
│  - Patches platform auth checks                   │
└─────────────────────────────────────────────────┘
```

### Component 1: t9-proxy (Rust cdylib → DLL)

**Crate**: `t9-proxy`
**Output**: DLL that masquerades as a system DLL the game loads

#### Responsibilities:
1. **IAT Hook Installation** — On `DLL_PROCESS_ATTACH`, walk the game's PE import table
   and replace Winsock function pointers with our stubs
2. **DNS Redirection** — `getaddrinfo` hook maps DW hostnames to `127.0.0.1`
3. **Host Blocking** — Return `WSAHOST_NOT_FOUND` for CDN/Blizzard/Activision hosts
4. **Connection Routing** — `connect` hook routes to our local servers
5. **Data Interception** — `send`/`recv` hooks pass data to/from local server instances
6. **Integrity Bypass** — Equivalent Arxan patches for T9 (will require RE of the T9 binary)
7. **Platform Patches** — T9-specific auth/online-check patches

#### Key Dependencies:
- `windows-sys` — Win32 API bindings
- `pelite` or manual PE parsing — For IAT walking
- `retour` or manual hooking — For function detouring

#### T9 Hostname Mapping (needs verification via packet capture):
```
t9-pc-auth3.prod.demonware.net     → 127.0.0.1:3100  (auth)
t9-pc-lobby.prod.demonware.net     → 127.0.0.1:3200  (lobby)
t9-stun.us.demonware.net           → 127.0.0.1:3074  (stun UDP)
t9-stun.eu.demonware.net           → 127.0.0.1:3074  (stun UDP)
prod.umbrella.demonware.net        → 127.0.0.1:3300  (umbrella)
t9-fileshare.prod.schild.net       → 127.0.0.1:3400  (fileshare)
```
Note: Actual T9 hostnames need to be captured from the game binary or network traffic.

### Component 2: t9-server (Rust binary)

**Crate**: `t9-server`
**Output**: Standalone executable running the emulated DW backend

#### Server Implementations:

##### Auth3Server (TCP, HTTP+JSON)
- Listens on port 3100
- Receives POST /auth/ with JSON body containing `title_id`, `iv_seed`, `identity`, `extra_data`
- Generates fake auth_ticket with configurable username/user_id
- Returns HTTP 200 with JSON containing `client_ticket`, `server_ticket`, session keys
- Session key is static (0x1337... pattern, same as Shield)

##### LobbyServer (TCP, binary protocol)
- Listens on port 3200
- Binary framing: 4-byte size prefix, then payload
- Three-phase handshake:
  1. Client sends 0xC8 header → Server sends header_ack (type 0x81, proto 0xD2)
  2. Client sends 0x82 auth → Server derives keys, sends auth_done (type 0x83)
  3. Client sends 0x85 encrypted service calls → Server decrypts (AES), dispatches to service
- Each service call: `[size:u32][0xAB][0x85][msg_count:u32][seed:16B][encrypted_payload][hash:8B]`
- Decrypted payload: `[size:u32][0x86][service_id:u8][task_data...]`

##### Services (implement as trait objects):
```rust
trait DemonWareService: Send + Sync {
    fn id(&self) -> u8;
    fn name(&self) -> &str;
    fn handle_task(&self, task_id: u8, data: &[u8], reply: &mut ReplyBuilder);
}
```

Priority implementation order:
1. **bdDML (27)** — Essential, handles data management
2. **bdTitleUtilities (12)** — Essential, title-specific config
3. **bdObjectStore (193)** — Essential, object storage
4. **bdAnticheat (38)** — Must ACK all challenges (empty replies)
5. **bdProfiles (8)** — Player profiles
6. **bdStats (4)** — Stats tracking
7. All others — Stub with empty success replies initially

##### StunServer (UDP)
- Listens on port 3074
- Handles type 30 (IP discovery) → returns 127.0.0.1:3074
- Handles type 20 (NAT discovery) → returns 127.0.0.1:3074 + server addr
- Critical for multiplayer connectivity

##### UmbrellaServer (TCP, HTTP)
- Listens on port 3300
- Stub — accepts all requests, returns 200 OK
- Handles telemetry/analytics that the game sends

##### FileshareServer (TCP)
- Listens on port 3400
- Handles user-generated content (emblems, clips, screenshots)
- Can be stubbed initially

#### Key Dependencies:
- `tokio` — Async runtime for server
- `hyper` or `axum` — HTTP server (auth3, umbrella)
- `aes` + `cbc` — AES-CBC decryption for lobby protocol
- `serde_json` — Auth3 JSON protocol
- `base64` — Ticket encoding

### Component 3: t9-tools CLI integration

Add to existing `bocw-mod-tools`:
- `bocw_mod_tools server start` — Launch t9-server
- `bocw_mod_tools server stop` — Stop server
- `bocw_mod_tools proxy install` — Copy proxy DLL to game directory
- `bocw_mod_tools proxy remove` — Restore original DLL
- `bocw_mod_tools launch` — Start server + launch game with proxy

---

## T9-Specific Research Needed

### Must-Do Before Implementation:
1. **Capture T9 DemonWare hostnames** — Run the game with Wireshark/Fiddler to see
   which `*.demonware.net` / `*.schild.net` addresses it resolves
2. **Identify proxy DLL target** — Which DLL does BOCW load that we can proxy?
   (BO4 uses XInput9_1_0.dll; T9 may use a different one)
3. **Find T9 Arxan signatures** — The integrity check patterns will differ from BO4
4. **Find T9 platform patches** — The auth/BattleNet bypass addresses are T9-specific
5. **Verify lobby protocol** — T9 may use a slightly different handshake or encryption

### Research Methods:
- **Static analysis**: Open `BlackOpsColdWar.exe` with a disassembler, search for
  DemonWare hostname strings, trace the auth flow
- **Dynamic analysis**: Use Wireshark to capture DNS queries and TCP connections
  on game startup (before it times out)
- **String extraction**: Use our existing zone tools to find DW-related strings
  in the game binary

---

## Multiplayer / Friends Support

### LAN Play (Phase 1):
- All players run t9-server locally
- Use virtual LAN (ZeroTier/Tailscale) to connect machines
- STUN server returns the VPN IP instead of 127.0.0.1
- Lobby server handles basic matchmaking

### Dedicated Server (Phase 2):
- One machine runs t9-server as the hub
- All players' proxy DLLs point to that machine's IP instead of 127.0.0.1
- Server handles auth for all players, assigns unique IDs
- Requires config file: `t9-server.toml` with bind address, player list, etc.

### Custom Maps (Phase 3):
- Leverage existing zone file tools to create/modify .ff files
- Fileshare server distributes custom zone files to connected players
- Zone hot-loading via the game's existing fastfile system

---

## File Structure

```
bocw-mod-tools/
├── Cargo.toml              (workspace root)
├── crates/
│   ├── t9-proxy/           (cdylib → proxy DLL)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs      (DLL entry, IAT hooking)
│   │       ├── hooks.rs    (Winsock hook stubs)
│   │       ├── patches.rs  (Arxan + platform patches)
│   │       └── config.rs   (Server addresses, settings)
│   │
│   ├── t9-server/          (DemonWare emulator server)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs
│   │       ├── auth3.rs    (Auth3 HTTP/JSON server)
│   │       ├── lobby.rs    (Lobby binary protocol server)
│   │       ├── stun.rs     (STUN UDP server)
│   │       ├── umbrella.rs (Umbrella HTTP stub)
│   │       ├── fileshare.rs
│   │       ├── crypto.rs   (AES, key derivation)
│   │       ├── protocol.rs (Byte/bit buffer, framing)
│   │       └── services/
│   │           ├── mod.rs
│   │           ├── anticheat.rs
│   │           ├── dml.rs
│   │           ├── profiles.rs
│   │           ├── stats.rs
│   │           ├── title_utilities.rs
│   │           ├── object_store.rs
│   │           └── ... (stubs for rest)
│   │
│   └── t9-common/          (shared types/protocol)
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs
│           ├── byte_buffer.rs
│           ├── bit_buffer.rs
│           └── data_types.rs
│
├── src/                    (existing bocw-mod-tools)
│   ├── main.rs
│   ├── lib.rs
│   └── ...
│
└── docs/
    └── demonware-emulator-architecture.md  (this file)
```

---

## Implementation Phases

### Phase 1: Foundation (Current)
- [x] Zone file decompression/recompression
- [x] String patching pipeline
- [x] Backup/restore system
- [ ] **T9 binary analysis** — Find DW hostnames, proxy DLL target, patch addresses
- [ ] **t9-server scaffold** — Auth3 + Lobby + STUN servers with stub responses
- [ ] **t9-proxy scaffold** — IAT hooking, DNS redirect, host blocking

### Phase 2: Solo Offline Play
- [ ] Complete auth3 flow — Generate valid tickets
- [ ] Lobby handshake — Three-phase binary protocol
- [ ] Essential services — bdDML, bdTitleUtilities, bdObjectStore, bdAnticheat
- [ ] Arxan bypass — T9-specific integrity patches
- [ ] Platform patches — Auth/online-check bypasses
- [ ] Integration — `bocw_mod_tools launch` command

### Phase 3: LAN Multiplayer
- [ ] STUN server with real IP reflection
- [ ] Lobby server with multi-player support
- [ ] ZeroTier/Tailscale integration guide
- [ ] Config for dedicated server mode

### Phase 4: Custom Content
- [ ] Custom map loading via zone file injection
- [ ] Fileshare server for content distribution
- [ ] Mod compatibility with custom maps
