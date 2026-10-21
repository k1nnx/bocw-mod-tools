//! T9 Proxy DLL — masquerades as XINPUT9_1_0.dll
//!
//! When placed in the BOCW game directory as `XINPUT9_1_0.dll`, the game loads this
//! DLL before the system version. On attach we:
//!   1. Allocate a dev console window for live logging
//!   2. Load the real XInput from the system directory and forward exports
//!   3. Hook the game's Winsock imports (IAT patching)
//!   4. Redirect DemonWare hosts that `t9-server` implements to the local emulator
//!   5. Block other DemonWare hosts and the Activision/Blizzard CDN hosts

#[cfg(windows)]
mod hooks;
pub mod routing;

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

            // Load real XInput from the system directory
            match load_real_xinput() {
                Some((handle, name)) => {
                    REAL_XINPUT = Some(handle);
                    tracing::info!("Real {} loaded from the system directory", name);
                }
                None => tracing::warn!("Could not load real XInput — controller input may not work"),
            }

            // Install Winsock IAT hooks
            match hooks::install_hooks() {
                Ok(hooked) => {
                    tracing::info!("Installed {} Winsock IAT hooks", hooked.len());
                    print_safety_status(&hooked);
                }
                Err(e) => tracing::error!("Failed to install hooks: {}", e),
            }

            tracing::info!("Proxy DLL ready — see [hooks]/[audit] lines for every routing decision");

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

#[cfg(windows)]
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
║   {YELLOW}Mode:{RESET}{CYAN}       EXPERIMENTAL — routing not game-verified       ║
║   {YELLOW}Proxy:{RESET}{CYAN}      XINPUT9_1_0.dll (Winsock IAT hooks)            ║
║   {YELLOW}Redirect:{RESET}{CYAN}   login/lobby/STUN/umbrella → 127.0.0.1         ║
║   {YELLOW}Blocked:{RESET}{CYAN}    other DemonWare hosts, Blizzard CDN           ║
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

#[cfg(windows)]
fn print_safety_status(hooked: &[&str]) {
    const GREEN: &str = "\x1b[32m";
    const YELLOW: &str = "\x1b[33m";
    const RED: &str = "\x1b[31m";
    const BOLD: &str = "\x1b[1m";
    const RESET: &str = "\x1b[0m";

    println!("\n{GREEN}{BOLD}  ── Routing Status ──{RESET}");

    let has = |name: &str| hooked.contains(&name);
    let dns_hooked = has("getaddrinfo") || has("GetAddrInfoW") || has("gethostbyname");
    let send_hooked = has("connect") || has("sendto");

    if dns_hooked && send_hooked {
        println!("  {GREEN}[ok]{RESET} IAT hooks installed: {}", hooked.join(", "));
    } else {
        println!(
            "  {RED}[!!]{RESET} Incomplete hooks ({}): redirection needs a DNS hook and connect/sendto",
            if hooked.is_empty() { "none".to_string() } else { hooked.join(", ") }
        );
    }

    println!(
        "  {YELLOW}[--]{RESET} Blocked: DemonWare hosts without a local service, plus {} CDN entries",
        t9_common::dw_protocol::hostnames::BLOCKED_HOSTS.len()
    );
    println!(
        "  {YELLOW}[--]{RESET} Unhooked APIs (WSAConnect, ConnectEx, GetAddrInfoExW, IPv6) are not covered"
    );
    println!("  {YELLOW}[--]{RESET} Connection audit: routing decisions are logged below");
    println!();
}

// ─── Real XInput Loading ────────────────────────────────────────────────────

#[cfg(windows)]
unsafe fn load_real_xinput() -> Option<(windows_sys::Win32::Foundation::HMODULE, &'static str)> {
    use windows_sys::Win32::System::LibraryLoader::LoadLibraryA;
    use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryA;

    // We masquerade as XINPUT9_1_0.dll, so load the REAL one by full path from
    // the system directory (a bare name would load this DLL again).
    let mut dir = [0u8; 260];
    let len = GetSystemDirectoryA(dir.as_mut_ptr(), dir.len() as u32) as usize;
    if len == 0 || len >= dir.len() {
        return None;
    }
    let dir = String::from_utf8_lossy(&dir[..len]).into_owned();

    // Try the exact name first (what the game imports), then fallbacks.
    for name in ["XInput9_1_0.dll", "XInput1_4.dll", "XInput1_3.dll"] {
        let path = format!("{}\\{}\0", dir, name);
        let handle = LoadLibraryA(path.as_ptr());
        if handle != 0 {
            return Some((handle, name));
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
