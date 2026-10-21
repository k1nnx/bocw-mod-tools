# BOCW Mod Tools

Experimental Rust tools for researching and modifying **Call of Duty: Black Ops Cold War (T9)** fastfiles, plus an unfinished local DemonWare emulator and Windows XInput proxy prototype.

> [!CAUTION]
> This repository is a development prototype, not a ready-to-use mod or a verified offline server. Use it only with a lawfully owned copy of the game and only in an isolated/offline test setup. The proxy does **not** prove that every game connection is blocked or redirected. Remove modifications and use Steam's **Verify integrity of game files** before using official services.

No game binaries, fastfiles, Oodle DLLs, extracted assets, credentials, or backup data belong in this repository.

## Current status

Status as of **2026-10-21**:

| Area | State |
| --- | --- |
| Rust workspace | Builds successfully on Windows; `cargo test --workspace` passes 7 tests |
| Fastfile tooling | TAFF header parsing, Oodle block decompression/recompression, extraction, search, and in-place patch primitives are implemented |
| Safety tooling | Backups, tracked restores, status, and read-only diagnostics have unit/integration coverage |
| Mod pipeline | Binary/string patch application exists; script/HUD injection is not connected end to end |
| `t9-server` | Localhost-only login, lobby, STUN, and umbrella prototype; most DemonWare behavior is stubbed or inferred |
| `t9-proxy` | Windows IAT-hooking prototype; target DLL and routing behavior still require validation against the current game build |
| In-game validation | No successful authentication, campaign load, or complete offline session has been established |

The most important limitation is documented in [`docs/connection-diagnostics.md`](docs/connection-diagnostics.md): compiling and passing tests does not establish that the game works or that traffic cannot reach official services.

## Workspace layout

```text
.
├── src/                    # Main bocw_mod_tools library and CLI
├── crates/
│   ├── t9-common/          # Shared protocol buffers, IDs, hostnames, and ports
│   ├── t9-server/          # Experimental localhost DemonWare services
│   └── t9-proxy/           # Windows XInput proxy and Winsock IAT hooks
├── tests/                  # CLI safety integration tests
├── mods/                   # Early sample mod data (currently not CLI-compatible)
└── docs/                   # Architecture, binary findings, and recovery notes
```

Generated `target/` output and `extracted/` game data are intentionally ignored.

## Development setup

### Requirements

- Windows x64
- Rust with the MSVC toolchain
- Visual Studio C++ Build Tools, if required by the Rust toolchain
- A legally installed Steam copy of Black Ops Cold War for integration testing
- The game's own `oo2core_8_win64.dll` for Oodle operations; it is loaded from the selected game directory and is not redistributed

### Build and test

```powershell
git clone https://github.com/k1nnx/bocw-mod-tools.git
cd bocw-mod-tools

cargo build --workspace
cargo test --workspace
cargo build --workspace --release
```

Release outputs are written under `target/release/`, including:

- `bocw_mod_tools.exe`
- `t9-server.exe`
- `t9_proxy.dll`

### Select the game directory

The CLI uses the `BOCW_GAME_DIR` environment variable, its built-in default Steam path, or the global `--game-dir` option.

```powershell
$env:BOCW_GAME_DIR = 'D:\SteamLibrary\steamapps\common\Call of Duty Black Ops Cold War'

# Start with read-only diagnostics.
cargo run -- doctor
cargo run -- info
cargo run -- zones --category zombies
cargo run -- inspect zm_common
```

Run `cargo run -- --help` for the complete command list.

## Safety model

The CLI distinguishes tracked project changes from a genuinely stock installation:

- `doctor`, `status`, `info`, `zones`, and `inspect` are intended as read-only diagnostics.
- `extract` writes derived game data to `extracted/`; never commit or redistribute it.
- `backup` writes `.bocw_backups` inside the selected game directory.
- `apply`, `restore`, and `clean` can write game fastfiles.
- `proxy install` and `proxy remove` can write game-local XInput DLLs.
- `verify` compares files only with this tool's backups. It is **not** equivalent to Steam verification.
- `clean` restores only tracked fastfiles and deliberately does not delete unknown DLLs.

Always close the game before modifying files. Do not bypass the offline check unless you understand the consequences. Before official play, remove known modifications, inspect extra DLLs, and verify the installation through Steam.

## Architecture notes

The repository contains two related but independently useful efforts:

1. **Fastfile/mod tools** — parse T9 TAFF headers, call the game's Oodle library, inspect/extract data, apply same-size binary or string patches, and retain backups.
2. **Offline service prototype** — redirect selected Winsock calls through an XInput proxy to localhost services implemented by `t9-server`.

Read these before changing the emulator path:

- [`docs/connection-diagnostics.md`](docs/connection-diagnostics.md) — observed failure state and safe recovery guidance
- [`docs/t9-demonware-findings.md`](docs/t9-demonware-findings.md) — static binary-analysis findings
- [`docs/demonware-emulator-architecture.md`](docs/demonware-emulator-architecture.md) — original design and roadmap; some details are now stale

## Claude Opus 4.6 handoff

Recommended first pass:

1. Run `cargo test --workspace` and preserve the existing safety behavior.
2. Treat all emulator/proxy claims as hypotheses until demonstrated with controlled runtime evidence.
3. Reconcile the XInput target inconsistency before installing anything: analysis notes and crate metadata mention `XInput1_0.dll`, while the current installer, tests, and parts of the proxy use `XINPUT9_1_0.dll`.
4. Add synthetic protocol tests or sanitized captures for auth/lobby behavior; do not commit game files, account data, tokens, or copyrighted captures.
5. Make the sample under `mods/no_weapon_rarities/` match the current `mod.toml` and `PatchSet` schemas, or replace it with a synthetic fixture.
6. Connect script/HUD generation to a tested injection path; `apply` currently generates a GSC file but only processes patch sets.
7. Add real-format round-trip tests using minimal synthetic fixtures before trusting recompressed fastfiles.
8. Validate proxy routing narrowly. The server currently exposes only a subset of the local endpoints referenced by the proxy, and most DemonWare services return prototype responses.

Useful code entry points:

- CLI dispatch: [`src/main.rs`](src/main.rs)
- Fastfile parsing/recompression: [`src/zone.rs`](src/zone.rs), [`src/taff.rs`](src/taff.rs)
- Backup and mutation safeguards: [`src/backup.rs`](src/backup.rs)
- Mod schema and HUD generation: [`src/mod_manager.rs`](src/mod_manager.rs)
- Proxy hooks: [`crates/t9-proxy/src/hooks.rs`](crates/t9-proxy/src/hooks.rs)
- Local services: [`crates/t9-server/src/main.rs`](crates/t9-server/src/main.rs)

## Legal and provenance

This project is unaffiliated with Activision, Treyarch, Demonware, Valve, or Microsoft. Call of Duty and related marks belong to their respective owners. No license has been selected for this private repository; do not redistribute it or third-party assets without the owner's permission.

The publication audit and handoff documentation were prepared with OpenCode using GPT-5.6-Sol. Existing source authorship is not asserted by that disclosure.
