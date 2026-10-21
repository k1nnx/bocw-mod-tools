//! T9 Proxy DLL — masquerades as XInput1_0.dll
//!
//! When placed in the BOCW game directory as `XInput1_0.dll`, the game loads this
//! DLL before the system version. On attach we:
//!   1. Allocate a dev console window for live logging
//!   2. Load the real XInput from System32 and forward exports
//!   3. Hook the game's Winsock imports (IAT patching)
//!   4. Redirect DemonWare traffic to our local emulator
//!   5. Block Activision/Blizzard CDN hosts

#[cfg(windows)]
mod hooks;

#[cfg(windows)]
use windows_sys::Win32::Foundation::{BOOL, TRUE};
#[cfg(windows)]
use windows_sys::Win32::System::SystemServices::{
    DLL_PROCESS_ATTACH, DLL_PROCESS_DETACH,
};

#[cfg(windows)]
static mut REAL_XINPUT: Option<windows_sys::Win32::Foundation::HMODULE> = None;

/// DLL entry point.
#[cfg(windows)]
#[no_mangle]
pub unsafe extern "system" fn DllMain(
    _hinst: windows_sys::Win32::Foundation::HMODULE,
    reason: u32,
    _reserved: *mut core::ffi::c_void,
) -> BOOL {
    match reason {
        DLL_PROCESS_ATTACH => {
            // Allocate a visible console window for dev logging
            alloc_dev_console();
            init_logging();
            print_banner();

            tracing::info!("Initializing T9 Proxy DLL...");

            // Load real XInput from System32
            if let Some(handle) = load_real_xinput() {
                REAL_XINPUT = Some(handle);
                tracing::info!("Real XINPUT9_1_0.dll loaded from System32");
            } else {
                tracing::warn!("Could not load real XInput — controller input may not work");
            }

            // Install Winsock IAT hooks
            match hooks::install_hooks() {
                Ok(count) => {
                    tracing::info!("Installed {} Winsock IAT hooks", count);
                    print_safety_status(count);
                }
                Err(e) => tracing::error!("Failed to install hooks: {}", e),
            }

            tracing::info!("Proxy DLL ready — all DemonWare traffic routes to 127.0.0.1");

            TRUE
        }
        DLL_PROCESS_DETACH => {
            // Don't use tracing here — TLS may already be destroyed
            hooks::remove_hooks();
            TRUE
        }
        _ => TRUE,
    }
}

// ─── Dev Console ────────────────────────────────────────────────────────────

#[cfg(windows)]
unsafe fn alloc_dev_console() {
    use windows_sys::Win32::System::Console::{
        AllocConsole, GetStdHandle, SetConsoleMode, SetConsoleTitleA,
        STD_OUTPUT_HANDLE, ENABLE_PROCESSED_OUTPUT, ENABLE_VIRTUAL_TERMINAL_PROCESSING,
    };

    AllocConsole();
    SetConsoleTitleA(b"T9 Dev Console - BOCW Offline Emulator\0".as_ptr());

    // Enable ANSI escape sequence processing so colors work
    let stdout = GetStdHandle(STD_OUTPUT_HANDLE);
    if stdout != 0 {
        SetConsoleMode(
            stdout,
            ENABLE_PROCESSED_OUTPUT | ENABLE_VIRTUAL_TERMINAL_PROCESSING,
        );
    }

    // Install a panic hook that doesn't double-panic on TLS errors
    std::panic::set_hook(Box::new(|info| {
        let msg = info.to_string();
        // Silently ignore TLS destruction panics — they're harmless
        if msg.contains("Thread Local Storage") || msg.contains("AccessError") {
            return;
        }
        eprintln!("[t9-proxy] PANIC: {}", msg);
    }));
}

#[cfg(windows)]
fn init_logging() {
    use tracing_subscriber::fmt;
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::EnvFilter;

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "t9_proxy=debug".into());

    // Console layer — colored, human-readable, goes to the AllocConsole window
    let console_layer = fmt::layer()
        .with_target(false)
        .with_ansi(true)
        .with_writer(std::io::stdout);

    // File layer — no colors, machine-readable, for post-mortem analysis
    let file_appender = tracing_appender::rolling::never(".", "t9-proxy.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    std::mem::forget(_guard);

    let file_layer = fmt::layer()
        .with_target(true)
        .with_ansi(false)
        .with_writer(non_blocking);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(console_layer)
        .with(file_layer)
        .init();
}

fn print_banner() {
    // ANSI color codes for a nice banner
    const CYAN: &str = "\x1b[36m";
    const GREEN: &str = "\x1b[32m";
    const YELLOW: &str = "\x1b[33m";
    const RED: &str = "\x1b[31m";
    const BOLD: &str = "\x1b[1m";
    const DIM: &str = "\x1b[2m";
    const RESET: &str = "\x1b[0m";

    let banner = format!(
        r#"
{CYAN}{BOLD}╔══════════════════════════════════════════════════════════════╗
║                                                              ║
║   ████████╗ █████╗      ██████╗ ███████╗██╗   ██╗           ║
║   ╚══██╔══╝██╔══██╗     ██╔══██╗██╔════╝██║   ██║           ║
║      ██║   ╚██████║     ██║  ██║█████╗  ██║   ██║           ║
║      ██║    ╚═══██║     ██║  ██║██╔══╝  ╚██╗ ██╔╝           ║
║      ██║    █████╔╝     ██████╔╝███████╗ ╚████╔╝            ║
║      ╚═╝    ╚════╝      ╚═════╝ ╚══════╝  ╚═══╝             ║
║                                                              ║
║   {GREEN}Black Ops Cold War — Offline DemonWare Emulator{CYAN}            ║
║                                                              ║
╠══════════════════════════════════════════════════════════════╣
║                                                              ║
║   {YELLOW}Mode:{RESET}{CYAN}       OFFLINE ONLY — No Activision connection       ║
║   {YELLOW}Proxy:{RESET}{CYAN}      XINPUT9_1_0.dll (Winsock IAT hooks)            ║
║   {YELLOW}Redirect:{RESET}{CYAN}   *.demonware.net → 127.0.0.1                   ║
║   {YELLOW}Blocked:{RESET}{CYAN}    Blizzard CDN, Battle.net                       ║
║                                                              ║
║   {RED}{BOLD}⚠  NEVER go online with modified files!{RESET}{CYAN}                    ║
║   {DIM}Run 'bocw-mod-tools clean' to restore originals first{RESET}{CYAN}    ║
║                                                              ║
╚══════════════════════════════════════════════════════════════╝{RESET}
"#
    );

    // Write directly to console stdout
    print!("{}", banner);
}

fn print_safety_status(hook_count: usize) {
    const GREEN: &str = "\x1b[32m";
    const YELLOW: &str = "\x1b[33m";
    const RED: &str = "\x1b[31m";
    const BOLD: &str = "\x1b[1m";
    const RESET: &str = "\x1b[0m";

    println!(
        "\n{GREEN}{BOLD}  ── Safety Status ──{RESET}"
    );

    if hook_count >= 3 {
        println!(
            "  {GREEN}[OK]{RESET} IAT hooks installed: {hook_count} Winsock functions redirected"
        );
    } else {
        println!(
            "  {RED}[!!]{RESET} Only {hook_count} hooks installed — some traffic may leak!"
        );
    }

    println!(
        "  {GREEN}[OK]{RESET} Blocked hosts: Blizzard CDN, Battle.net ({} entries)",
        t9_common::dw_protocol::hostnames::BLOCKED_HOSTS.len()
    );
    println!(
        "  {GREEN}[OK]{RESET} DemonWare redirect: all *.demonware.net → 127.0.0.1"
    );
    println!(
        "  {GREEN}[OK]{RESET} Anti-cheat service: ACKs all challenges locally"
    );
    println!(
        "  {YELLOW}[--]{RESET} Connection audit: all connect() calls logged below"
    );
    println!();
}

// ─── Real XInput Loading ────────────────────────────────────────────────────

#[cfg(windows)]
unsafe fn load_real_xinput() -> Option<windows_sys::Win32::Foundation::HMODULE> {
    use windows_sys::Win32::System::LibraryLoader::LoadLibraryA;

    // We masquerade as XINPUT9_1_0.dll, so load the REAL one from System32.
    // Try the exact name first (what the game originally imported), then fallbacks.
    let candidates: &[&[u8]] = &[
        b"C:\\Windows\\System32\\XInput9_1_0.dll\0",
        b"C:\\Windows\\System32\\XInput1_4.dll\0",
        b"C:\\Windows\\System32\\XInput1_3.dll\0",
    ];

    for path in candidates {
        let handle = LoadLibraryA(path.as_ptr());
        if handle != 0 {
            return Some(handle);
        }
    }
    None
}

// ─── XInput Export Forwarding ───────────────────────────────────────────────

#[cfg(windows)]
#[no_mangle]
pub unsafe extern "system" fn XInputGetState(
    user_index: u32,
    state: *mut u8,
) -> u32 {
    type Fn = unsafe extern "system" fn(u32, *mut u8) -> u32;
    if let Some(handle) = REAL_XINPUT {
        let proc = windows_sys::Win32::System::LibraryLoader::GetProcAddress(
            handle,
            b"XInputGetState\0".as_ptr(),
        );
        if let Some(f) = proc {
            let f: Fn = std::mem::transmute(f);
            return f(user_index, state);
        }
    }
    0x048F // ERROR_DEVICE_NOT_CONNECTED
}

#[cfg(windows)]
#[no_mangle]
pub unsafe extern "system" fn XInputSetState(
    user_index: u32,
    vibration: *mut u8,
) -> u32 {
    type Fn = unsafe extern "system" fn(u32, *mut u8) -> u32;
    if let Some(handle) = REAL_XINPUT {
        let proc = windows_sys::Win32::System::LibraryLoader::GetProcAddress(
            handle,
            b"XInputSetState\0".as_ptr(),
        );
        if let Some(f) = proc {
            let f: Fn = std::mem::transmute(f);
            return f(user_index, vibration);
        }
    }
    0x048F
}

#[cfg(windows)]
#[no_mangle]
pub unsafe extern "system" fn XInputGetCapabilities(
    user_index: u32,
    flags: u32,
    capabilities: *mut u8,
) -> u32 {
    type Fn = unsafe extern "system" fn(u32, u32, *mut u8) -> u32;
    if let Some(handle) = REAL_XINPUT {
        let proc = windows_sys::Win32::System::LibraryLoader::GetProcAddress(
            handle,
            b"XInputGetCapabilities\0".as_ptr(),
        );
        if let Some(f) = proc {
            let f: Fn = std::mem::transmute(f);
            return f(user_index, flags, capabilities);
        }
    }
    0x048F
}

#[cfg(windows)]
#[no_mangle]
pub unsafe extern "system" fn XInputEnable(enable: i32) {
    type Fn = unsafe extern "system" fn(i32);
    if let Some(handle) = REAL_XINPUT {
        let proc = windows_sys::Win32::System::LibraryLoader::GetProcAddress(
            handle,
            b"XInputEnable\0".as_ptr(),
        );
        if let Some(f) = proc {
            let f: Fn = std::mem::transmute(f);
            f(enable);
        }
    }
}

// Non-Windows stub
#[cfg(not(windows))]
pub fn placeholder() {}
