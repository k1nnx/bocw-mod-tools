# T9 (BOCW) DemonWare Binary Analysis Findings

## Game Binary Analyzed
- File: `BlackOpsColdWar.exe` (303 MB)
- Source path in binary: `e:\t9_release\code\src\...`
- Game codename: **navyblue** (found at exe offset 0xD4936E0)

## DemonWare Hostnames Found in Binary

### Format String Pattern
The game builds hostnames dynamically using:
- Format: `%s%c%s%c%s%c%s` assembling `{codename}-pc-{service}.{env}.demonware.net`
- Components found in binary: `auth3`, `navyblue`, `net`, `https`
- Environment names: `cert`, `prod`, `global`

### Confirmed Hostnames

#### Login Service (HTTP REST API)
- `loginservice.%s.demonware.net` (template, %s = environment)
- `%s-%s-loginservice.%s.demonware.net` (template: codename-platform-loginservice.env)
- `loginservice-stg.dev.demonware.net` (staging)
- `loginservice-loadtest.prod.demonware.net` (load test)
- **Production**: `loginservice.prod.demonware.net` or `navyblue-pc-loginservice.prod.demonware.net`

#### STUN Servers (UDP)
- `stun.us.demonware.net`
- `stun.eu.demonware.net`
- `stun.jp.demonware.net`
- `stun.au.demonware.net`
- Note: T9 uses `stun.*` NOT `ops4-stun.*` (BO4 leftover strings also present in binary)

#### Umbrella (HTTP analytics/telemetry)
- `%s.umbrella.demonware.net` (template)
- `prod.umbrella.demonware.net`
- `umbrella.%s.cod.qq.com` (Chinese region)

#### ObjectStore (HTTP REST)
- `objectstore.dev.demonware.net`
- `objectstore.cert.demonware.net`
- `objectstore.prod.demonware.net`
- API: `/v2/users/%s/objects/%s/?client=%s&...`

#### Exchange
- `exchange.demonware.net`

#### AE (Analytics Engine?)
- `ae.demonware.net`

#### CDN/Blocked Hosts (from BO4 reference, likely same for T9)
- `us.cdn.blizzard.com`
- `eu.cdn.blizzard.com`
- `us.actual.battle.net`
- `eu.actual.battle.net`
- `level3.blizzard.com`
- `blzddist1-a.akamaihd.net`
- `level3.ssl.blizzard.com`

### Lobby Service Gateway (LSG)
- Not a fixed hostname — returned dynamically in login service response as `lsg_endpoint`
- Uses binary protocol (same as BO4 lobby)
- Connected after successful authentication
- Game string: "Connected to LSG. Updating state to: REPORTING_EXTENDED_AUTH_INFO"

## Auth Flow (T9 differs from BO4)

T9 uses a REST-based login service rather than BO4's raw auth3 JSON:

1. Game resolves `loginservice.prod.demonware.net`
2. Sends HTTP requests to login API:
   - `GET /v1/login/{platform}/?titleID={id}&client={client}`
   - `GET /v1/login/?titleID={id}`
   - `GET /v1/login/resume/?titleID={id}&client={client}`
3. Login service responds with:
   - Auth tickets
   - `lsg_endpoint` — the lobby server address to connect to
   - Umbrella data
   - Uno platform data
   - Login queue info
4. Game connects to LSG for binary lobby protocol (same 3-phase handshake as BO4)

## DLL Import Analysis

### Proxy DLL Target: XINPUT9_1_0.dll

> **Correction (2026-10-21):** this section originally named `XInput1_0.dll`.
> That is treated as a misread: Windows ships no `XInput1_0.dll` (only
> `xinput1_1`–`xinput1_4` and `xinput9_1_0`), and a proxy installed as
> `XINPUT9_1_0.dll` was observed loading in the game process
> (`docs/connection-diagnostics.md`). The code now uses `XINPUT9_1_0.dll`
> everywhere (`t9_common::dw_protocol::proxy::DLL_NAME`). Re-check the import
> table of the current build before relying on this.

The game imports:
- `XInputSetState` from `XINPUT9_1_0.dll`
- `XInputGetState` from `XINPUT9_1_0.dll`

### Winsock Imports (what we hook)
From `WS2_32.dll`:
- `getaddrinfo`
- `inet_ntop`
- (other standard winsock functions)

### Other Key Imports
- `KERNEL32.dll` — standard Windows API
- `USER32.dll` — window management
- `ADVAPI32.dll` — crypto (`CryptEncrypt`)
- `WINMM.dll` — multimedia timer
- `IPHLPAPI.DLL` — network adapter info

## Key Strings Found

### Auth-Related
- `"auth3"` — auth service identifier
- `"navyblue"` — T9 codename for hostname construction
- `"DW-ABToken"` — DemonWare auth bearer token header
- `"bdAuth::validateResponseSignature"` implied (from auth flow)
- `"WARDEN_DBG: Generating battle.net UID"`
- `"Invalid use of ATVI dedi login on Battle.net build"`

### Service Names
- `"STORAGE"`, `"STATS"`, `"CHKPTSAVE"`, `"DLOG"`, `"FILESHARE"`,
  `"LPC"`, `"MARKETING"`, `"STORE"`, `"STORECOMM"`

### Error/Flow Strings
- `"Lobby DNS Lookup Error (status: %u)"`
- `"Error connecting to lobby (connectionID: %I64u) %s (%u)"`
- `"Failed to discover login service environment, can't connect to service"`
- `"Failed to retrieve antiCheat instance from lobbyService"`
- `"bdAntiCheat::reportExtendedAuthInfo()"`
- `"LOBBY_ROUTER_UPDATE_FAILED"`

## Comparison: T9 vs T8 (BO4)

| Feature | BO4 (T8) | BOCW (T9) |
|---------|----------|-----------|
| Codename | ops4 | navyblue |
| Proxy DLL | XInput9_1_0.dll | XINPUT9_1_0.dll (see correction above) |
| Auth | Raw auth3 JSON endpoint | REST login service API |
| Lobby hostname | Fixed: ops4-pc-lobby.prod.demonware.net | Dynamic: returned as lsg_endpoint |
| STUN prefix | ops4-stun.* | stun.* (no prefix) |
| Login flow | Direct auth ticket exchange | Multi-step REST flow with login tokens |
| Anti-cheat | bdAnticheat service | bdAnticheat + Arxan integrity checks |
