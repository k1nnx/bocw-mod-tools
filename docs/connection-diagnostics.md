# Stuck at “Connecting to Online Services”: findings and safe recovery

This repository contains two different projects: zone-file tools, and an **unfinished** local DemonWare emulator/proxy. `cargo test` verifies that Rust code compiles and limited tests pass; it does not establish that the game can authenticate, load campaign, or use online services. The emulator architecture itself describes missing auth, lobby and anti-tamper work. It is not a substitute for a real Activision account or the official service.

## What this machine showed (read-only, 2026-10-21)

- Steam install: `C:\Program Files (x86)\Steam\steamapps\common\Call of Duty Black Ops Cold War`.
- The game-local `XINPUT9_1_0.dll` matched this repo's `target/release/t9_proxy.dll` by SHA-256. `t9-proxy.log` records that the DLL ran and intercepted DNS. **There is no `XINPUT9_1_0.dll.bak` sibling**, so the old `proxy remove` implementation would have deleted it without knowing whether it was an original game file.
- `.bocw_backups/mod_state.json` lists `zone\zm_common.ff` as modified; the current file differs from its `.bocw_backups/zone/zm_common.ff` backup. We did not restore it.
- `t9-server` was running on ports 3100, 3200, 3300 (TCP) and 3074 (UDP) when checked. The old binary bound ports 3300 and 3074 to **all interfaces**, not just localhost. Changes to its source do not affect an already running binary: restart it only after you decide to stop using it. No process was stopped during this investigation.
- The log alone does **not** prove that all game traffic was redirected, that authentication worked, or that the official servers were down.

Run `cargo run -- doctor` for read-only status. `cargo run -- status` is also read-only. Neither command certifies that files are stock. `cargo run -- clean` **does write game files** (tracked zone restore); it now refuses to claim a clean install while game-local XInput DLLs remain. It deliberately does not delete those DLLs. `cargo run -- proxy remove` now refuses to delete an unbacked DLL; do not rely on it to clean this installation.

## If you want to play through official services

1. Close the game first. Back up saves/configuration and any game files you wish to preserve. **With your approval**, stop the specific emulator process and move the confirmed custom DLL *out of the game directory* to a private backup location (do not blindly delete an unknown DLL). The CLI does **not** do this automatically. If you made other modifications, review those as well.
2. Restore any modified zones from **known-good** backups, or use Steam to repair installed files. Steam: game > **Properties > Installed Files > Verify integrity of game files**. Steam verification may not remove extra non-Steam files, so check for extra local DLLs separately before launching. Do not regard `.bocw_backups` as proof of stock files.
3. Launch from Steam, with normal Steam online access, and check your Activision account is properly signed in/linked. Activision says online play requires an Activision account. Check the official [online services status](https://support.activision.com/onlineservices) and [Cold War connection guidance](https://support.activision.com/black-ops-cold-war/articles/connecting-to-a-black-ops-cold-war-game). Do not launch the official game with mods/proxy still in place.
4. If the original, verified game still gets stuck, follow [Activision support](https://support.activision.com/black-ops-cold-war/articles/connecting-to-a-black-ops-cold-war-game) and [Steam network troubleshooting](https://help.steampowered.com/en/faqs/view/669A-2F68-D1D1-A5EC); include the actual error code, platform status, and whether you can sign into your Activision account. Avoid sharing passwords, account cookies, or session tokens.

Sources: [Steam verification](https://help.steampowered.com/en/faqs/view/0C48-FCBD-DA71-93EB), [Activision account requirement](https://support.activision.com/black-ops-cold-war/articles/black-ops-cold-war-in-game-account-registration), [Steam game listing](https://store.steampowered.com/app/1985810). The local observations above were checked against files and process state directly; they may change after this date.
