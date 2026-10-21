# Windows client/server test handoff — 2026-10-21

**For Opus:** This is a test report, not a claim that Black Ops Cold War authenticates or works offline. Repository `main` was cloned at `62f5637` (`Merge pull request #1 from k1nnx/handoff-tasks`). Tests ran on Windows x64 using `cargo 1.85.0-nightly` / `rustc 1.85.0-nightly`, MSVC toolchain. The tester has a game install; Opus may not. Existing game files were **not** modified by this session. The owner chose synthetic-only testing rather than a real game launch on a networked machine.

## Outcome and process ownership

- `cargo build --workspace`, `cargo test --workspace`, and `cargo build --workspace --release`: **exit 0**; 41 passing Rust tests, no failures. These cover mostly synthetic fixtures, not gameplay.
- The pre-existing `t9-server.exe` at `C:\Users\john\Downloads\bocw-mod-tools\target\release\t9-server.exe` (PID **31544**) listened on `0.0.0.0:3300` TCP and `0.0.0.0:3074` UDP as well as localhost TCP 3100/3200. At the owner's explicit request it was stopped, by exact PID after confirming the executable path. The ports were then free.
- **New server**: `C:\Users\john\Downloads\forked\bocw-mod-tools\target\debug\t9-server.exe`, started with `RUST_LOG=t9_server=debug`, synthetic `USERNAME=LocalTester`, PID **8940**. It was still running at handoff; TCP **3100, 3200, 3300** and UDP **3074** were verified bound to `127.0.0.1` by that PID. Debug config was generated under ignored `target/debug/t9-server.toml`; it did not alter the game install. The debug log contains only our synthetic traffic.
- The actual game client was **not launched** this session. The user selected synthetic-only testing because no enforced outbound isolation was available (the shell is not Administrator and the game has modified content). Do **not** equate our localhost smoke client with an in-game validation.
- The installed game-local proxy is an **older build**, not this clone's new release binary; comparison below. It was not replaced. No game DLL, fastfile, backup, or mod was installed, restored, deleted, or overwritten.

## Reproduce the tests (Windows)

```powershell
cargo build --workspace
cargo test --workspace
cargo build --workspace --release
# Make sure ports 3100/3200/3300 TCP and 3074 UDP are available FIRST.
$env:RUST_LOG='t9_server=debug'
cargo run -p t9-server
# In another shell:
python tests/server_smoke_client.py
python tests/read_only_cli_smoke.py 'C:\Program Files (x86)\Steam\steamapps\common\Call of Duty Black Ops Cold War' docs/windows-cli-2026-10-21.txt
```

`server_smoke_client.py` talks only to 127.0.0.1, does not send account credentials, and deliberately exits **1** for reproduced protocol failures. Each socket has a **5-second response deadline**; this does not terminate the server. It never asserts that the game uses `/auth/` or TLS. `read_only_cli_smoke.py` reads game metadata and writes a log; do not use `apply`, `backup`, `clean`, `restore`, `proxy install/remove`, or `extract` on the existing install without reviewing it first. Do not enable DEBUG logging with real game traffic until request logging is made credential-safe (`crates/t9-server/src/auth.rs:47` prints up to 200 bytes of incoming requests).

## Synthetic client results (server PID 8940)

Full stdout: [`windows-server-smoke-2026-10-21.txt`](windows-server-smoke-2026-10-21.txt). Corresponding full debug trace: [`windows-server-runtime-2026-10-21.txt`](windows-server-runtime-2026-10-21.txt).

| Test | Result | Observed evidence |
| --- | --- | --- |
| REST login `GET /v1/login/steam/?titleID=27551&client=pc` | PASS | HTTP 200; JSON `lsg.host=127.0.0.1:3200`; base64 ticket decodes to 128 bytes; the ticket is never printed |
| Umbrella `GET /synthetic` | PASS | HTTP 200; `{}` body |
| STUN IP / NAT discovery | PASS | UDP reply types 31 (9 bytes) / 21 (15 bytes); reflected synthetic peer `127.0.0.1` |
| Lobby header + auth | PASS | A single-write synthetic header produced 26-byte 0x81 ack; synthetic auth produced 14-byte 0x83 reply |
| Lobby header split across two TCP writes | **FAIL** | Timed out without header ack; trace: `buffer underflow: tried to read past end`, then `Expected 0xAB, got 0x0`. TCP reads are not message boundaries; see `crates/t9-server/src/lobby.rs:40-55` and `:70-109` |
| Legacy `GET /auth/` | **FAIL** | Timed out on a complete HTTP request: `crates/t9-server/src/auth.rs:49-53` discards it waiting for a body. Not evidence that the real game uses this endpoint |
| Legacy `POST /auth/` with complete JSON in one write | **FAIL** | Timed out; the handler discards any read starting `POST /auth/`, even when it includes a complete body |
| Same POST, headers/body in separate writes | PASS | HTTP 200 JSON `code=700`: result depends on TCP segmentation; see `auth.rs:37-83` |
| Incomplete synthetic TLS record to login port | **FAIL** (TLS compatibility) | Listener replied in **plain HTTP/1.1 200 OK**, not TLS. The game might use HTTPS; this test makes no claim about which transport the actual game uses |

**Separate startup failure:** While PID 8940 owned all four ports, a second new `t9-server.exe` failed to bind **all** services (`os error 10048`), nevertheless printed three green TCP service indicators and `Server is running`, and exited **0**. `crates/t9-server/src/main.rs:35-84` logs spawned-service errors but `tokio::select` then returns `Ok(())`; `print_service_status` probes another process, not service ownership. Full transcript: [`windows-server-port-conflict-2026-10-21.txt`](windows-server-port-conflict-2026-10-21.txt). A temp copy with `bind_addr = "0.0.0.0"` correctly refused startup with **exit 1**: [`windows-server-bind-guard-2026-10-21.txt`](windows-server-bind-guard-2026-10-21.txt).

## Evidence only available on the machine with the game

Steam app ID **1985810**, Steam appmanifest `buildid` **23222070**; install `C:\Program Files (x86)\Steam\steamapps\common\Call of Duty Black Ops Cold War`; save/config directory reported by `um scan`: `C:\Users\john\Documents\Call of Duty Black Ops Cold War`. Main executable is **native x64**, not managed .NET: `um scan` misleadingly labels the install ".NET application" because `reportcodbug.exe` (the *reporting helper*) is managed x86; it found no anti-cheat or loader in this directory, which is not proof that launch/network play is safe. Windows exe version resources report `1.0.0.0` (not a reliable game build number). The raw Steam appmanifest was not copied to Git because it contains an account identifier.

| Game-local file (relative to install) | Bytes | SHA-256 | Note |
| --- | ---: | --- | --- |
| `BlackOpsColdWar.exe` | 317731832 | `93760CB587042DA50D861D3184BC9D6B6DF6FD58B735EAFFA2F5D9C1ACD17427` | native x64, present; not copied |
| `oo2core_8_win64.dll` | 1007616 | `D5440627BBDE1EF2D9730976DD90C5189D874FB7BB808669734E598CDFDBA8D9` | real Oodle codec, present; not copied to repo |
| `zone\zm_common.ff` | 23446592 | `0B44DEBF1B2AF67521B959287EF41F41107F72004FE64BDE3CE88F920099483B` | **modified** current zone; not copied |
| `.bocw_backups\zone\zm_common.ff` | 20749504 | `38748FB661E6672E0B613991CB5A8E4F93E49F563883BD6784959DCCCB01297F` | older backup snapshot; not proven Steam-stock; not copied |
| `XINPUT9_1_0.dll` | 1334784 | `6AB1E5CFC48DB902AD5B89667F6A2B7913413610CACA4607A5A0AAE9C67D099E` | equals **old clone's** `target/release/t9_proxy.dll`; **no `.bak` sibling** |
| `t9-proxy.log` | 15114 | `54FFF06B4CB592F23A77EC0364F0016699DB8E355B4A60A7C67A279D3500F7A8` | prior game run, text copied verbatim below |

This clone's `target/release/t9_proxy.dll` is **1339904 bytes, SHA-256 `076252D1A0BBDDB1460CD1A1AB1452E1B0D6179AE68EA970BFD42CDCF17561BA`**. A new server paired with the *installed old proxy* is not a test of new proxy routing. Do not use `proxy install`: no known original XInput backup exists; do not silently replace or delete the current game-local DLL.

`doctor/status`: `.bocw_backups/mod_state.json` reports `mods_active=true`, `active_mods=["no_weapon_rarities"]`, `modified_files=["zone\\zm_common.ff"]`, `last_applied=2026-10-21T17:31:48.991632+00:00`, `offline_verified=false`. A modified zone differs from its backup. **No backup manifest** is present; `verify` prints `No backup manifest found` and exits 0. This is *not* a successful file-integrity verification. The new CLI saw 92 `.ff` files (40 zombies, 28 core, 24 multiplayer). Name/size metadata, with **no** fastfile contents: [`windows-zone-file-inventory-2026-10-21.csv`](windows-zone-file-inventory-2026-10-21.csv).

`inspect zm_common` on the modified zone: TAFF version `0x64`, Oodle Kraken, platform 3 unencrypted, builder `TA-BM-SV-T9-25`, 463 blocks, decompressed size **121188103** bytes, 9 header chunks; full header metadata in [`windows-cli-2026-10-21.txt`](windows-cli-2026-10-21.txt). `inspect ww_zm_tungsten`: TAFF `0x64`, Kraken, builder `TA-BM-SV-T9-13`, one block, decompressed **938020** bytes. This is metadata only, not proof the game accepts either file.

**Real-Oodle read-only source round trips (new information for an agent without the game):** `test_recompress.exe` was run with `BOCW_GAME_DIR` pointing at the game to **read** the modified zone and Oodle DLL; it wrote and removed a temporary output under `%LOCALAPPDATA%\Temp\opencode`, not the game install. Its decompressed bytes matched exactly after recompression (121188103 bytes); original and recompressed both **23446592 bytes**, and it found `weapon_rarity_map_list` **0** times: [`windows-oodle-roundtrip-2026-10-21.txt`](windows-oodle-roundtrip-2026-10-21.txt). The same test on a temporary **copy of the backup snapshot**, plus a temporary Oodle DLL copy, also passed byte-for-byte on decompressed output; original **20749504**, recompressed **23446592** bytes (**113.0%** size), `weapon_rarity_map_list` **1** hit: [`windows-oodle-stock-backup-roundtrip-2026-10-21.txt`](windows-oodle-stock-backup-roundtrip-2026-10-21.txt). The temp fastfile outputs were removed, and the backup/DLL temp copies were cleaned up. These prove a local codec round trip, **not** game loading, file identity, absence of corruption elsewhere, or that the backup was actually stock.

### Pre-existing game-client proxy log (not a launch in this session)

The 165-line text log at [`windows-prior-game-proxy-2026-10-21.txt`](windows-prior-game-proxy-2026-10-21.txt) was copied **verbatim** after inspection (no credentials or payloads). From three previous starts it records the proxy loading the real system XInput DLL, finding the game's WS2_32 import descriptor at RVA `0x1bb1d000`, and installing **4** IAT hooks (ordinal #4, `getaddrinfo`, ordinal #52, `freeaddrinfo`). The only observed service lookup lines are `ops4-stun.{us,eu,au,jp}.demonware.net -> 127.0.0.1:3074`. **Important discrepancy:** `docs/t9-demonware-findings.md:25-30` asserted T9 uses `stun.*` and treated `ops4-stun.*` as BO4 leftovers; these prior *runtime* lines establish that this installation did request ops4-stun hostnames at least once. They do not prove which component requested them, authentication, that the newest proxy hooks work, or that all traffic was intercepted. The old log's `all DemonWare traffic routes to 127.0.0.1` banner is an unverified claim, not a finding. There are no `connect`, `sendto`, or auth-success records in this old log.

## Prioritized Opus actions / safety constraints

1. **Make TCP framing deterministic**: accumulate a full 4-byte size + bounded payload for each lobby packet; support short/coalesced reads and reject bogus lengths; test with fragmented and concatenated writes. For HTTP, accumulate headers + Content-Length, separate request paths from body parsing, and avoid logging auth headers/bodies.
2. **Fail server startup truthfully**: propagate bind/task errors as nonzero; verify ownership and binding per listener, including UDP, instead of treating any reachable local port as this server. Add collision tests and a failure-status test.
3. **Resolve transport and auth with controlled evidence**: static/game traces may expect TLS. Current login is plain HTTP, `/auth/` relies on split TCP reads, and auth ticket/service responses are synthetic. Do not claim an offline login based on localhost status alone.
4. **Reconcile runtime proxy coverage before game testing**: the installed proxy is the old binary. New routing tests operate on pure functions; game IAT hook installation is unverified. The hook source only intercepts the main module's IAT and lists `WSAConnect`, `ConnectEx`, `GetAddrInfoExW`, and IPv6 as not covered. Non-DemonWare hosts and unmarked destinations pass through to real Winsock, so the proxy is **not** an egress firewall. A game launch needs independently enforced network isolation and approval.
5. **Do not auto-clean this installation**: `mod_state.json` says modified; manifest absent; game-local proxy has no `.bak`; `clean`/`restore` affect game files and Steam verification does not remove unknown DLLs. Treat the backup as unverified. Work with synthetic fixtures until a deliberate, reversible lab is approved.
6. After a safe, isolated real game session is possible, capture *behavioral* evidence (game logs/UI state, exact loaded proxy hash, process-owned connection endpoints and hook list) without committing secrets or any proprietary fastfiles. Keep fastfile patching and script injection separate from login debugging; built GSC/HUD files are not loaded by the game yet.

## Text evidence index (all in `docs/`)

- [`windows-build-2026-10-21.txt`](windows-build-2026-10-21.txt) and [`windows-release-build-2026-10-21.txt`](windows-release-build-2026-10-21.txt): build stdout/stderr, exit codes.
- [`windows-tests-2026-10-21.txt`](windows-tests-2026-10-21.txt): 41-test Windows workspace transcript.
- [`windows-cli-2026-10-21.txt`](windows-cli-2026-10-21.txt): all eight read-only CLI commands and exits.
- [`windows-server-runtime-2026-10-21.txt`](windows-server-runtime-2026-10-21.txt): full debug trace of new server startup and synthetic requests (may contain synthetic request text; do not reuse DEBUG for actual credentials).
- [`windows-server-smoke-2026-10-21.txt`](windows-server-smoke-2026-10-21.txt): reproducible synthetic client output and four expected failures.
- [`windows-server-port-conflict-2026-10-21.txt`](windows-server-port-conflict-2026-10-21.txt), [`windows-server-bind-guard-2026-10-21.txt`](windows-server-bind-guard-2026-10-21.txt): startup failure evidence.
- [`windows-oodle-roundtrip-2026-10-21.txt`](windows-oodle-roundtrip-2026-10-21.txt), [`windows-oodle-stock-backup-roundtrip-2026-10-21.txt`](windows-oodle-stock-backup-roundtrip-2026-10-21.txt): real Oodle testing on modified zone and backup snapshot (no fastfile data).
- [`windows-prior-game-proxy-2026-10-21.txt`](windows-prior-game-proxy-2026-10-21.txt): the **older**, unedited game-client proxy log, not produced by this session.
- [`windows-zone-file-inventory-2026-10-21.csv`](windows-zone-file-inventory-2026-10-21.csv): game-local names and sizes only.

No executable, DLL, `.ff`, extracted assets, tickets, account secrets, game backup files, or network packet captures were added to Git.
