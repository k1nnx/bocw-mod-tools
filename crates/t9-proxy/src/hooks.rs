//! IAT-based Winsock hooks for redirecting DemonWare traffic.
//!
//! On DLL load, we walk the game's PE import table and replace Winsock function
//! pointers with our own stubs. Our stubs check if the target is a DemonWare
//! host and redirect to 127.0.0.1 (the local emulator). Non-DW traffic passes
//! through to the real Winsock functions.

#![allow(non_snake_case)]
#![allow(non_camel_case_types)]

use std::ffi::CStr;
use std::sync::Mutex;
use t9_common::dw_protocol::{hostnames, ports};

#[cfg(windows)]
use windows_sys::Win32::Foundation::HMODULE;
#[cfg(windows)]
use windows_sys::Win32::Networking::WinSock::{
    ADDRINFOA, AF_INET, SOCKADDR, SOCKADDR_IN, SOCKET, SOCKET_ERROR, WSAHOST_NOT_FOUND,
};
#[cfg(windows)]
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress, LoadLibraryA};
#[cfg(windows)]
use windows_sys::Win32::System::Memory::{VirtualProtect, PAGE_READWRITE};

// ─── PE structures (manually defined for reliable IAT walking) ──────────────

#[repr(C)]
struct IMAGE_DOS_HEADER {
    e_magic: u16,
    _pad: [u16; 29],
    e_lfanew: i32,
}

#[repr(C)]
struct IMAGE_NT_HEADERS64 {
    Signature: u32,
    FileHeader: IMAGE_FILE_HEADER,
    OptionalHeader: IMAGE_OPTIONAL_HEADER64,
}

#[repr(C)]
struct IMAGE_FILE_HEADER {
    Machine: u16,
    NumberOfSections: u16,
    TimeDateStamp: u32,
    PointerToSymbolTable: u32,
    NumberOfSymbols: u32,
    SizeOfOptionalHeader: u16,
    Characteristics: u16,
}

#[repr(C)]
struct IMAGE_OPTIONAL_HEADER64 {
    Magic: u16,                   // 0
    MajorLinkerVersion: u8,       // 2
    MinorLinkerVersion: u8,       // 3
    SizeOfCode: u32,              // 4
    SizeOfInitializedData: u32,   // 8
    SizeOfUninitializedData: u32, // 12
    AddressOfEntryPoint: u32,     // 16
    BaseOfCode: u32,              // 20
    ImageBase: u64,               // 24
    SectionAlignment: u32,        // 32
    FileAlignment: u32,           // 36
    MajorOSVersion: u16,          // 40
    MinorOSVersion: u16,          // 42
    MajorImageVersion: u16,       // 44
    MinorImageVersion: u16,       // 46
    MajorSubsystemVersion: u16,   // 48
    MinorSubsystemVersion: u16,   // 50
    Win32VersionValue: u32,       // 52
    SizeOfImage: u32,             // 56
    SizeOfHeaders: u32,           // 60
    CheckSum: u32,                // 64
    Subsystem: u16,               // 68
    DllCharacteristics: u16,      // 70
    SizeOfStackReserve: u64,      // 72
    SizeOfStackCommit: u64,       // 80
    SizeOfHeapReserve: u64,       // 88
    SizeOfHeapCommit: u64,        // 96
    LoaderFlags: u32,             // 104
    NumberOfRvaAndSizes: u32,     // 108
    DataDirectory: [IMAGE_DATA_DIRECTORY; 16], // 112
}

#[repr(C)]
#[derive(Clone, Copy)]
struct IMAGE_DATA_DIRECTORY {
    VirtualAddress: u32,
    Size: u32,
}

#[repr(C)]
struct IMAGE_IMPORT_DESCRIPTOR {
    OriginalFirstThunk: u32, // RVA to INT (Import Name Table)
    TimeDateStamp: u32,
    ForwarderChain: u32,
    Name: u32,               // RVA to DLL name string
    FirstThunk: u32,         // RVA to IAT (Import Address Table)
}

#[repr(C)]
struct IMAGE_IMPORT_BY_NAME {
    Hint: u16,
    Name: [u8; 1], // variable-length
}

const IMAGE_DIRECTORY_ENTRY_IMPORT: usize = 1;
const IMAGE_ORDINAL_FLAG64: u64 = 0x8000000000000000;

// ─── Hook state ─────────────────────────────────────────────────────────────

struct HookEntry {
    iat_addr: *mut usize,
    original_fn: usize,
}

// Safety: we only access these from DllMain (serialized by loader lock)
// and from hook functions (which just read the original pointers)
unsafe impl Send for HookEntry {}
unsafe impl Sync for HookEntry {}

static HOOKS: Mutex<Vec<HookEntry>> = Mutex::new(Vec::new());

// Original Winsock function pointers
static mut REAL_CONNECT: Option<unsafe extern "system" fn(SOCKET, *const SOCKADDR, i32) -> i32> = None;
static mut REAL_GETADDRINFO: Option<unsafe extern "system" fn(*const u8, *const u8, *const ADDRINFOA, *mut *mut ADDRINFOA) -> i32> = None;
static mut REAL_FREEADDRINFO: Option<unsafe extern "system" fn(*mut ADDRINFOA)> = None;
static mut REAL_GETHOSTBYNAME: Option<unsafe extern "system" fn(*const u8) -> *mut u8> = None;

// Handle to real WS2_32.dll for fallback calls
static mut WS2_32_HANDLE: HMODULE = 0;

// ─── Public API ─────────────────────────────────────────────────────────────

pub fn install_hooks() -> Result<usize, String> {
    unsafe { install_hooks_impl() }
}

pub fn remove_hooks() {
    unsafe { remove_hooks_impl() }
}

// ─── Implementation ─────────────────────────────────────────────────────────

#[cfg(windows)]
unsafe fn install_hooks_impl() -> Result<usize, String> {
    // Load real WS2_32.dll so we can forward non-DW calls
    let ws2 = LoadLibraryA(b"ws2_32.dll\0".as_ptr());
    if ws2 == 0 {
        return Err("Failed to load ws2_32.dll".into());
    }
    WS2_32_HANDLE = ws2;

    // Save real function pointers from the actual WS2_32
    REAL_CONNECT = load_fn(ws2, b"connect\0");
    REAL_GETADDRINFO = load_fn(ws2, b"getaddrinfo\0");
    REAL_FREEADDRINFO = load_fn(ws2, b"freeaddrinfo\0");
    REAL_GETHOSTBYNAME = load_fn(ws2, b"gethostbyname\0");

    tracing::info!("[hooks] Loaded real WS2_32.dll at {:#x}", ws2 as usize);

    // Get the game's main module (exe)
    let game_module = GetModuleHandleA(std::ptr::null());
    if game_module == 0 {
        return Err("Failed to get game module handle".into());
    }
    tracing::info!("[hooks] Game module base: {:#x}", game_module as usize);

    let base = game_module as *const u8;

    // Parse PE headers
    let dos = &*(base as *const IMAGE_DOS_HEADER);
    if dos.e_magic != 0x5A4D {
        return Err("Invalid DOS signature".into());
    }

    let nt = &*(base.offset(dos.e_lfanew as isize) as *const IMAGE_NT_HEADERS64);
    if nt.Signature != 0x00004550 {
        return Err("Invalid NT signature".into());
    }

    let import_dir = nt.OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT];
    if import_dir.VirtualAddress == 0 {
        return Err("No import directory found".into());
    }

    tracing::info!(
        "[hooks] Import directory RVA: {:#x}, size: {:#x}",
        import_dir.VirtualAddress,
        import_dir.Size
    );

    // Walk import descriptors
    let mut import_desc = base.add(import_dir.VirtualAddress as usize) as *const IMAGE_IMPORT_DESCRIPTOR;
    let mut hook_count = 0u32;

    loop {
        let desc = &*import_desc;
        if desc.Name == 0 && desc.FirstThunk == 0 {
            break; // null terminator
        }

        let dll_name_ptr = base.add(desc.Name as usize) as *const i8;
        let dll_name = match CStr::from_ptr(dll_name_ptr).to_str() {
            Ok(s) => s,
            Err(_) => {
                import_desc = import_desc.add(1);
                continue;
            }
        };

        // We only hook WS2_32.dll imports
        if !dll_name.eq_ignore_ascii_case("WS2_32.dll") {
            import_desc = import_desc.add(1);
            continue;
        }

        tracing::info!("[hooks] Found WS2_32.dll import descriptor");

        // Walk the INT (Import Name Table) and IAT in parallel
        let int_base = if desc.OriginalFirstThunk != 0 {
            desc.OriginalFirstThunk
        } else {
            desc.FirstThunk
        };

        let mut int_entry = base.add(int_base as usize) as *const u64;
        let mut iat_entry = base.add(desc.FirstThunk as usize) as *mut usize;

        loop {
            let thunk = *int_entry;
            if thunk == 0 {
                break;
            }

            // Log and handle ordinal imports
            if thunk & IMAGE_ORDINAL_FLAG64 != 0 {
                let ordinal = (thunk & 0xFFFF) as u16;
                tracing::debug!("[hooks]   WS2_32 ordinal import: #{}", ordinal);

                // connect is ordinal 4 in ws2_32.dll
                let hook_fn: Option<usize> = match ordinal {
                    4 => Some(hooked_connect as usize),   // connect
                    52 => Some(hooked_gethostbyname as usize), // gethostbyname
                    _ => None,
                };

                if let Some(new_fn) = hook_fn {
                    let original = *iat_entry;
                    let mut old_protect: u32 = 0;
                    let ok = VirtualProtect(
                        iat_entry as *mut _,
                        std::mem::size_of::<usize>(),
                        PAGE_READWRITE,
                        &mut old_protect,
                    );
                    if ok != 0 {
                        *iat_entry = new_fn;
                        VirtualProtect(
                            iat_entry as *mut _,
                            std::mem::size_of::<usize>(),
                            old_protect,
                            &mut old_protect,
                        );
                        if let Ok(mut hooks) = HOOKS.lock() {
                            hooks.push(HookEntry {
                                iat_addr: iat_entry,
                                original_fn: original,
                            });
                        }
                        tracing::info!(
                            "[hooks] Hooked ordinal #{} @ IAT {:#x}",
                            ordinal, iat_entry as usize
                        );
                        hook_count += 1;
                    }
                }

                int_entry = int_entry.add(1);
                iat_entry = iat_entry.add(1);
                continue;
            }

            let import_by_name = base.add(thunk as usize) as *const IMAGE_IMPORT_BY_NAME;
            let func_name_ptr = &(*import_by_name).Name as *const u8 as *const i8;
            let func_name = match CStr::from_ptr(func_name_ptr).to_str() {
                Ok(s) => s,
                Err(_) => {
                    int_entry = int_entry.add(1);
                    iat_entry = iat_entry.add(1);
                    continue;
                }
            };

            tracing::debug!("[hooks]   WS2_32 import: {}", func_name);

            let hook_fn: Option<usize> = match func_name {
                "connect" => Some(hooked_connect as usize),
                "getaddrinfo" => Some(hooked_getaddrinfo as usize),
                "freeaddrinfo" => Some(hooked_freeaddrinfo as usize),
                "gethostbyname" => Some(hooked_gethostbyname as usize),
                _ => None,
            };

            if let Some(new_fn) = hook_fn {
                let original = *iat_entry;

                // Make the IAT entry writable
                let mut old_protect: u32 = 0;
                let ok = VirtualProtect(
                    iat_entry as *mut _,
                    std::mem::size_of::<usize>(),
                    PAGE_READWRITE,
                    &mut old_protect,
                );
                if ok == 0 {
                    tracing::warn!("[hooks] VirtualProtect failed for {}", func_name);
                    int_entry = int_entry.add(1);
                    iat_entry = iat_entry.add(1);
                    continue;
                }

                // Patch the IAT
                *iat_entry = new_fn;

                // Restore protection
                VirtualProtect(
                    iat_entry as *mut _,
                    std::mem::size_of::<usize>(),
                    old_protect,
                    &mut old_protect,
                );

                // Save for unhooking
                if let Ok(mut hooks) = HOOKS.lock() {
                    hooks.push(HookEntry {
                        iat_addr: iat_entry,
                        original_fn: original,
                    });
                }

                tracing::info!(
                    "[hooks] Hooked {} @ IAT {:#x}: {:#x} -> {:#x}",
                    func_name,
                    iat_entry as usize,
                    original,
                    new_fn
                );
                hook_count += 1;
            }

            int_entry = int_entry.add(1);
            iat_entry = iat_entry.add(1);
        }

        import_desc = import_desc.add(1);
    }

    tracing::info!("[hooks] Installed {} IAT hooks", hook_count);
    Ok(hook_count as usize)
}

#[cfg(not(windows))]
unsafe fn install_hooks_impl() -> Result<usize, String> {
    Ok(0)
}

#[cfg(windows)]
unsafe fn remove_hooks_impl() {
    // Wrap in catch_unwind — this runs during DLL_PROCESS_DETACH when TLS
    // may already be destroyed, so any tracing call could panic
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if let Ok(mut hooks) = HOOKS.lock() {
            for entry in hooks.drain(..) {
                let mut old_protect: u32 = 0;
                let ok = VirtualProtect(
                    entry.iat_addr as *mut _,
                    std::mem::size_of::<usize>(),
                    PAGE_READWRITE,
                    &mut old_protect,
                );
                if ok != 0 {
                    *entry.iat_addr = entry.original_fn;
                    VirtualProtect(
                        entry.iat_addr as *mut _,
                        std::mem::size_of::<usize>(),
                        old_protect,
                        &mut old_protect,
                    );
                }
            }
        }
    }));
}

#[cfg(not(windows))]
unsafe fn remove_hooks_impl() {}

// ─── Helper ─────────────────────────────────────────────────────────────────

#[cfg(windows)]
unsafe fn load_fn<T>(module: HMODULE, name: &[u8]) -> Option<T> {
    let proc = GetProcAddress(module, name.as_ptr());
    proc.map(|p| std::mem::transmute_copy(&p))
}

// ─── Hook functions ─────────────────────────────────────────────────────────

/// Hooked `connect` — intercepts connections to DemonWare IPs and redirects to localhost.
#[cfg(windows)]
unsafe extern "system" fn hooked_connect(
    socket: SOCKET,
    name: *const SOCKADDR,
    namelen: i32,
) -> i32 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        hooked_connect_impl(socket, name, namelen)
    }))
    .unwrap_or(SOCKET_ERROR)
}

#[cfg(windows)]
unsafe fn hooked_connect_impl(
    socket: SOCKET,
    name: *const SOCKADDR,
    namelen: i32,
) -> i32 {
    let real = match REAL_CONNECT {
        Some(f) => f,
        None => return SOCKET_ERROR,
    };

    if namelen >= std::mem::size_of::<SOCKADDR_IN>() as i32 {
        let addr_in = &*(name as *const SOCKADDR_IN);
        if addr_in.sin_family == AF_INET as u16 {
            let ip_bytes = addr_in.sin_addr.S_un.S_addr.to_ne_bytes();
            let port = u16::from_be(addr_in.sin_port);

            let redirect_port = match port {
                3074 => Some(ports::STUN),
                3100 => Some(ports::LOGIN_SERVICE),
                3200 => Some(ports::LOBBY),
                3300 => Some(ports::UMBRELLA),
                3400 => Some(ports::FILESHARE),
                3500 => Some(ports::OBJECT_STORE),
                80 | 443 => {
                    if ip_bytes[0] != 127 {
                        tracing::info!(
                            "[hooks] connect: HTTP(S) port {} to {}.{}.{}.{} - redirecting to auth",
                            port, ip_bytes[0], ip_bytes[1], ip_bytes[2], ip_bytes[3]
                        );
                        Some(ports::LOGIN_SERVICE)
                    } else {
                        None
                    }
                }
                _ => None,
            };

            if let Some(local_port) = redirect_port {
                let mut redirected = *addr_in;
                redirected.sin_addr.S_un.S_addr = u32::from_ne_bytes([127, 0, 0, 1]);
                redirected.sin_port = local_port.to_be();

                tracing::info!(
                    "[hooks] connect: {}.{}.{}.{}:{} -> 127.0.0.1:{}",
                    ip_bytes[0], ip_bytes[1], ip_bytes[2], ip_bytes[3],
                    port,
                    local_port
                );

                return real(
                    socket,
                    &redirected as *const SOCKADDR_IN as *const SOCKADDR,
                    std::mem::size_of::<SOCKADDR_IN>() as i32,
                );
            }
        }
    }

    if namelen >= std::mem::size_of::<SOCKADDR_IN>() as i32 {
        let addr_in = &*(name as *const SOCKADDR_IN);
        if addr_in.sin_family == AF_INET as u16 {
            let ip = addr_in.sin_addr.S_un.S_addr.to_ne_bytes();
            let port = u16::from_be(addr_in.sin_port);
            tracing::debug!(
                "[audit] connect passthrough: {}.{}.{}.{}:{}",
                ip[0], ip[1], ip[2], ip[3], port
            );
        }
    }
    real(socket, name, namelen)
}

/// Hooked `getaddrinfo` — resolves DemonWare hostnames to localhost.
#[cfg(windows)]
unsafe extern "system" fn hooked_getaddrinfo(
    node: *const u8,
    service: *const u8,
    hints: *const ADDRINFOA,
    result: *mut *mut ADDRINFOA,
) -> i32 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        hooked_getaddrinfo_impl(node, service, hints, result)
    }))
    .unwrap_or(WSAHOST_NOT_FOUND)
}

#[cfg(windows)]
unsafe fn hooked_getaddrinfo_impl(
    node: *const u8,
    service: *const u8,
    hints: *const ADDRINFOA,
    result: *mut *mut ADDRINFOA,
) -> i32 {
    let real = match REAL_GETADDRINFO {
        Some(f) => f,
        None => return SOCKET_ERROR,
    };

    if !node.is_null() {
        if let Ok(hostname) = CStr::from_ptr(node as *const i8).to_str() {
            if let Some(resolution) = resolve_dw_host(hostname) {
                match resolution {
                    DwResolution::Redirect(port) => {
                        tracing::info!(
                            "[hooks] getaddrinfo: {} -> 127.0.0.1:{}",
                            hostname,
                            port
                        );
                        let localhost = b"127.0.0.1\0";
                        return real(localhost.as_ptr(), service, hints, result);
                    }
                    DwResolution::Block => {
                        tracing::info!("[hooks] getaddrinfo: BLOCKED {}", hostname);
                        return WSAHOST_NOT_FOUND;
                    }
                }
            } else {
                tracing::debug!("[audit] getaddrinfo passthrough: {}", hostname);
            }
        }
    }

    real(node, service, hints, result)
}

/// Hooked `freeaddrinfo` — just forwards to real implementation.
#[cfg(windows)]
unsafe extern "system" fn hooked_freeaddrinfo(ai: *mut ADDRINFOA) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if let Some(real) = REAL_FREEADDRINFO {
            real(ai);
        }
    }));
}

/// Hooked `gethostbyname` — legacy DNS resolution, redirects DW hosts.
#[cfg(windows)]
unsafe extern "system" fn hooked_gethostbyname(name: *const u8) -> *mut u8 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        hooked_gethostbyname_impl(name)
    }))
    .unwrap_or(std::ptr::null_mut())
}

#[cfg(windows)]
unsafe fn hooked_gethostbyname_impl(name: *const u8) -> *mut u8 {
    let real = match REAL_GETHOSTBYNAME {
        Some(f) => f,
        None => return std::ptr::null_mut(),
    };

    if !name.is_null() {
        if let Ok(hostname) = CStr::from_ptr(name as *const i8).to_str() {
            if let Some(resolution) = resolve_dw_host(hostname) {
                match resolution {
                    DwResolution::Redirect(_port) => {
                        tracing::info!(
                            "[hooks] gethostbyname: {} -> 127.0.0.1",
                            hostname
                        );
                        let localhost = b"127.0.0.1\0";
                        return real(localhost.as_ptr());
                    }
                    DwResolution::Block => {
                        tracing::info!("[hooks] gethostbyname: BLOCKED {}", hostname);
                        return std::ptr::null_mut();
                    }
                }
            }
        }
    }

    real(name)
}

// ─── Host resolution ────────────────────────────────────────────────────────

enum DwResolution {
    Redirect(u16),
    Block,
}

fn resolve_dw_host(hostname: &str) -> Option<DwResolution> {
    if hostnames::is_blocked_host(hostname) {
        return Some(DwResolution::Block);
    }

    if hostname.contains("loginservice") && hostname.contains("demonware.net") {
        return Some(DwResolution::Redirect(ports::LOGIN_SERVICE));
    }
    if hostname.contains("auth3") && hostname.contains("demonware.net") {
        return Some(DwResolution::Redirect(ports::LOGIN_SERVICE));
    }
    if hostname.contains("stun") && hostname.contains("demonware.net") {
        return Some(DwResolution::Redirect(ports::STUN));
    }
    if hostname.contains("umbrella") && hostname.contains("demonware.net") {
        return Some(DwResolution::Redirect(ports::UMBRELLA));
    }
    if hostname.contains("objectstore") && hostname.contains("demonware.net") {
        return Some(DwResolution::Redirect(ports::OBJECT_STORE));
    }
    if hostname.contains("exchange") && hostname.contains("demonware.net") {
        return Some(DwResolution::Redirect(ports::LOBBY));
    }
    if hostname.contains("ae.demonware.net") {
        return Some(DwResolution::Redirect(ports::UMBRELLA));
    }
    if hostname.contains("fileshare") {
        return Some(DwResolution::Redirect(ports::FILESHARE));
    }

    // Catch-all for any demonware.net host
    if hostnames::is_demonware_host(hostname) {
        tracing::warn!("[hooks] Unknown DW host: {} — routing to lobby", hostname);
        return Some(DwResolution::Redirect(ports::LOBBY));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dw_host_resolution() {
        // Login service
        assert!(matches!(
            resolve_dw_host("loginservice.prod.demonware.net"),
            Some(DwResolution::Redirect(3100))
        ));
        assert!(matches!(
            resolve_dw_host("navyblue-pc-loginservice.prod.demonware.net"),
            Some(DwResolution::Redirect(3100))
        ));

        // STUN
        assert!(matches!(
            resolve_dw_host("stun.us.demonware.net"),
            Some(DwResolution::Redirect(3074))
        ));

        // Umbrella
        assert!(matches!(
            resolve_dw_host("prod.umbrella.demonware.net"),
            Some(DwResolution::Redirect(3300))
        ));

        // Blocked hosts
        assert!(matches!(
            resolve_dw_host("us.cdn.blizzard.com"),
            Some(DwResolution::Block)
        ));

        // Non-DW hosts pass through (None)
        assert!(resolve_dw_host("store.steampowered.com").is_none());
        assert!(resolve_dw_host("google.com").is_none());
    }
}
