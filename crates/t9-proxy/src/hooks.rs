//! IAT-based Winsock hooks for redirecting DemonWare traffic.
//!
//! On DLL load, we walk the game's PE import table and replace Winsock function
//! pointers with our own stubs. The DNS stubs resolve DemonWare hosts to
//! per-service marker addresses (or fail the lookup); the `connect`/`sendto`
//! stubs rewrite only those markers to the local emulator. Routing rules live
//! in `routing.rs`; everything else passes through to the real Winsock.

#![allow(non_snake_case)]
#![allow(non_camel_case_types)]

use std::ffi::CStr;
use std::sync::Mutex;

use crate::routing::{self, HostRoute};

use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::Networking::WinSock::{
    ADDRINFOA, ADDRINFOW, AF_INET, SOCKADDR, SOCKADDR_IN, SOCKET, SOCKET_ERROR, WSAHOST_NOT_FOUND,
};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress, LoadLibraryA};
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

type ConnectFn = unsafe extern "system" fn(SOCKET, *const SOCKADDR, i32) -> i32;
type SendToFn = unsafe extern "system" fn(SOCKET, *const u8, i32, i32, *const SOCKADDR, i32) -> i32;
type GetAddrInfoFn =
    unsafe extern "system" fn(*const u8, *const u8, *const ADDRINFOA, *mut *mut ADDRINFOA) -> i32;
type GetAddrInfoWFn =
    unsafe extern "system" fn(*const u16, *const u16, *const ADDRINFOW, *mut *mut ADDRINFOW) -> i32;
type FreeAddrInfoFn = unsafe extern "system" fn(*mut ADDRINFOA);
type GetHostByNameFn = unsafe extern "system" fn(*const u8) -> *mut u8;

// Original Winsock function pointers
static mut REAL_CONNECT: Option<ConnectFn> = None;
static mut REAL_SENDTO: Option<SendToFn> = None;
static mut REAL_GETADDRINFO: Option<GetAddrInfoFn> = None;
static mut REAL_GETADDRINFOW: Option<GetAddrInfoWFn> = None;
static mut REAL_FREEADDRINFO: Option<FreeAddrInfoFn> = None;
static mut REAL_GETHOSTBYNAME: Option<GetHostByNameFn> = None;

// Handle to real WS2_32.dll for fallback calls
static mut WS2_32_HANDLE: HMODULE = 0;

// ─── Public API ─────────────────────────────────────────────────────────────

/// Install the hooks and return the names of the imports that were patched.
pub fn install_hooks() -> Result<Vec<&'static str>, String> {
    unsafe { install_hooks_impl() }
}

pub fn remove_hooks() {
    unsafe { remove_hooks_impl() }
}

// ─── Implementation ─────────────────────────────────────────────────────────

fn hook_for_name(name: &str) -> Option<(&'static str, usize)> {
    Some(match name {
        "connect" => ("connect", hooked_connect as *const () as usize),
        "sendto" => ("sendto", hooked_sendto as *const () as usize),
        "getaddrinfo" => ("getaddrinfo", hooked_getaddrinfo as *const () as usize),
        "GetAddrInfoW" => ("GetAddrInfoW", hooked_getaddrinfow as *const () as usize),
        "freeaddrinfo" => ("freeaddrinfo", hooked_freeaddrinfo as *const () as usize),
        "gethostbyname" => ("gethostbyname", hooked_gethostbyname as *const () as usize),
        _ => return None,
    })
}

/// WS2_32 exports a few classic functions at fixed ordinals.
fn hook_for_ordinal(ordinal: u16) -> Option<(&'static str, usize)> {
    match ordinal {
        4 => hook_for_name("connect"),
        20 => hook_for_name("sendto"),
        52 => hook_for_name("gethostbyname"),
        _ => None,
    }
}

unsafe fn install_hooks_impl() -> Result<Vec<&'static str>, String> {
    // Load real WS2_32.dll so we can forward non-DW calls
    let ws2 = LoadLibraryA(b"ws2_32.dll\0".as_ptr());
    if ws2 == 0 {
        return Err("Failed to load ws2_32.dll".into());
    }
    WS2_32_HANDLE = ws2;

    // Save real function pointers from the actual WS2_32
    REAL_CONNECT = load_fn(ws2, b"connect\0");
    REAL_SENDTO = load_fn(ws2, b"sendto\0");
    REAL_GETADDRINFO = load_fn(ws2, b"getaddrinfo\0");
    REAL_GETADDRINFOW = load_fn(ws2, b"GetAddrInfoW\0");
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
    let mut hooked = Vec::new();

    loop {
        let desc = &*import_desc;
        if desc.Name == 0 && desc.FirstThunk == 0 {
            break; // null terminator
        }
        import_desc = import_desc.add(1);

        let dll_name_ptr = base.add(desc.Name as usize) as *const i8;
        // We only hook WS2_32.dll imports
        match CStr::from_ptr(dll_name_ptr).to_str() {
            Ok(name) if name.eq_ignore_ascii_case("WS2_32.dll") => {}
            _ => continue,
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

        while *int_entry != 0 {
            let thunk = *int_entry;

            let hook = if thunk & IMAGE_ORDINAL_FLAG64 != 0 {
                let ordinal = (thunk & 0xFFFF) as u16;
                tracing::debug!("[hooks]   WS2_32 ordinal import: #{}", ordinal);
                hook_for_ordinal(ordinal)
            } else {
                let import_by_name = base.add(thunk as usize) as *const IMAGE_IMPORT_BY_NAME;
                let func_name_ptr = &(*import_by_name).Name as *const u8 as *const i8;
                match CStr::from_ptr(func_name_ptr).to_str() {
                    Ok(func_name) => {
                        tracing::debug!("[hooks]   WS2_32 import: {}", func_name);
                        hook_for_name(func_name)
                    }
                    Err(_) => None,
                }
            };

            if let Some((name, new_fn)) = hook {
                if patch_iat_entry(iat_entry, new_fn) {
                    tracing::info!("[hooks] Hooked {} @ IAT {:#x}", name, iat_entry as usize);
                    hooked.push(name);
                } else {
                    tracing::warn!("[hooks] VirtualProtect failed for {}", name);
                }
            }

            int_entry = int_entry.add(1);
            iat_entry = iat_entry.add(1);
        }
    }

    tracing::info!("[hooks] Installed {} IAT hooks: {:?}", hooked.len(), hooked);
    Ok(hooked)
}

unsafe fn patch_iat_entry(iat_entry: *mut usize, new_fn: usize) -> bool {
    let size = std::mem::size_of::<usize>();
    let mut old_protect: u32 = 0;
    if VirtualProtect(iat_entry as *const _, size, PAGE_READWRITE, &mut old_protect) == 0 {
        return false;
    }

    let original = *iat_entry;
    *iat_entry = new_fn;
    VirtualProtect(iat_entry as *const _, size, old_protect, &mut old_protect);

    if let Ok(mut hooks) = HOOKS.lock() {
        hooks.push(HookEntry {
            iat_addr: iat_entry,
            original_fn: original,
        });
    }
    true
}

unsafe fn remove_hooks_impl() {
    // Wrap in catch_unwind — this runs during DLL_PROCESS_DETACH when TLS
    // may already be destroyed, so any tracing call could panic
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if let Ok(mut hooks) = HOOKS.lock() {
            for entry in hooks.drain(..) {
                let mut old_protect: u32 = 0;
                let ok = VirtualProtect(
                    entry.iat_addr as *const _,
                    std::mem::size_of::<usize>(),
                    PAGE_READWRITE,
                    &mut old_protect,
                );
                if ok != 0 {
                    *entry.iat_addr = entry.original_fn;
                    VirtualProtect(
                        entry.iat_addr as *const _,
                        std::mem::size_of::<usize>(),
                        old_protect,
                        &mut old_protect,
                    );
                }
            }
        }
    }));
}

// ─── Helper ─────────────────────────────────────────────────────────────────

unsafe fn load_fn<T>(module: HMODULE, name: &[u8]) -> Option<T> {
    let proc = GetProcAddress(module, name.as_ptr());
    proc.map(|p| std::mem::transmute_copy(&p))
}

/// If `name` is one of our marker addresses, return the emulator address to
/// use instead. Every other destination is left alone.
unsafe fn redirected_sockaddr(api: &str, name: *const SOCKADDR, namelen: i32) -> Option<SOCKADDR_IN> {
    if name.is_null() || namelen < std::mem::size_of::<SOCKADDR_IN>() as i32 {
        return None;
    }
    let addr_in = &*(name as *const SOCKADDR_IN);
    if addr_in.sin_family != AF_INET {
        return None;
    }

    let ip = addr_in.sin_addr.S_un.S_addr.to_ne_bytes();
    let port = u16::from_be(addr_in.sin_port);
    let Some((new_ip, new_port)) = routing::rewrite_destination(ip, port) else {
        tracing::debug!(
            "[audit] {} passthrough: {}.{}.{}.{}:{}",
            api, ip[0], ip[1], ip[2], ip[3], port
        );
        return None;
    };

    let mut redirected = *addr_in;
    redirected.sin_addr.S_un.S_addr = u32::from_ne_bytes(new_ip);
    redirected.sin_port = new_port.to_be();
    tracing::info!(
        "[hooks] {}: {}.{}.{}.{}:{} -> {}.{}.{}.{}:{}",
        api, ip[0], ip[1], ip[2], ip[3], port,
        new_ip[0], new_ip[1], new_ip[2], new_ip[3], new_port
    );
    Some(redirected)
}

fn route_lookup(api: &str, hostname: &str) -> Option<HostRoute> {
    let route = routing::route_host(hostname);
    match route {
        Some(HostRoute::Redirect(service)) => {
            let [a, b, c, d] = routing::marker_ip(service);
            tracing::info!(
                "[hooks] {}: {} -> {}.{}.{}.{} ({})",
                api, hostname, a, b, c, d, service.name()
            );
        }
        Some(HostRoute::Block) => tracing::info!("[hooks] {}: BLOCKED {}", api, hostname),
        None => tracing::debug!("[audit] {} passthrough: {}", api, hostname),
    }
    route
}

// ─── Hook functions ─────────────────────────────────────────────────────────

/// Hooked `connect` — rewrites marker destinations to the local emulator.
unsafe extern "system" fn hooked_connect(
    socket: SOCKET,
    name: *const SOCKADDR,
    namelen: i32,
) -> i32 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let Some(real) = REAL_CONNECT else {
            return SOCKET_ERROR;
        };
        match redirected_sockaddr("connect", name, namelen) {
            Some(redirected) => real(
                socket,
                &redirected as *const SOCKADDR_IN as *const SOCKADDR,
                std::mem::size_of::<SOCKADDR_IN>() as i32,
            ),
            None => real(socket, name, namelen),
        }
    }))
    .unwrap_or(SOCKET_ERROR)
}

/// Hooked `sendto` — UDP traffic (STUN) to marker destinations.
unsafe extern "system" fn hooked_sendto(
    socket: SOCKET,
    buf: *const u8,
    len: i32,
    flags: i32,
    to: *const SOCKADDR,
    tolen: i32,
) -> i32 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let Some(real) = REAL_SENDTO else {
            return SOCKET_ERROR;
        };
        match redirected_sockaddr("sendto", to, tolen) {
            Some(redirected) => real(
                socket,
                buf,
                len,
                flags,
                &redirected as *const SOCKADDR_IN as *const SOCKADDR,
                std::mem::size_of::<SOCKADDR_IN>() as i32,
            ),
            None => real(socket, buf, len, flags, to, tolen),
        }
    }))
    .unwrap_or(SOCKET_ERROR)
}

/// Hooked `getaddrinfo` — resolves DemonWare hostnames to marker addresses.
unsafe extern "system" fn hooked_getaddrinfo(
    node: *const u8,
    service: *const u8,
    hints: *const ADDRINFOA,
    result: *mut *mut ADDRINFOA,
) -> i32 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let Some(real) = REAL_GETADDRINFO else {
            return WSAHOST_NOT_FOUND;
        };
        if !node.is_null() {
            if let Ok(hostname) = CStr::from_ptr(node as *const i8).to_str() {
                match route_lookup("getaddrinfo", hostname) {
                    Some(HostRoute::Redirect(svc)) => {
                        let marker = routing::marker_host_cstr(svc);
                        return real(marker.as_ptr(), service, hints, result);
                    }
                    Some(HostRoute::Block) => return WSAHOST_NOT_FOUND,
                    None => {}
                }
            }
        }
        real(node, service, hints, result)
    }))
    .unwrap_or(WSAHOST_NOT_FOUND)
}

/// Hooked `GetAddrInfoW` — wide-string variant of `getaddrinfo`.
unsafe extern "system" fn hooked_getaddrinfow(
    node: *const u16,
    service: *const u16,
    hints: *const ADDRINFOW,
    result: *mut *mut ADDRINFOW,
) -> i32 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let Some(real) = REAL_GETADDRINFOW else {
            return WSAHOST_NOT_FOUND;
        };
        if let Some(hostname) = wide_to_string(node) {
            match route_lookup("GetAddrInfoW", &hostname) {
                Some(HostRoute::Redirect(svc)) => {
                    let marker: Vec<u16> = String::from_utf8_lossy(&routing::marker_host_cstr(svc))
                        .encode_utf16()
                        .collect();
                    return real(marker.as_ptr(), service, hints, result);
                }
                Some(HostRoute::Block) => return WSAHOST_NOT_FOUND,
                None => {}
            }
        }
        real(node, service, hints, result)
    }))
    .unwrap_or(WSAHOST_NOT_FOUND)
}

unsafe fn wide_to_string(ptr: *const u16) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let mut len = 0;
    while *ptr.add(len) != 0 {
        len += 1;
    }
    String::from_utf16(std::slice::from_raw_parts(ptr, len)).ok()
}

/// Hooked `freeaddrinfo` — just forwards to real implementation.
unsafe extern "system" fn hooked_freeaddrinfo(ai: *mut ADDRINFOA) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if let Some(real) = REAL_FREEADDRINFO {
            real(ai);
        }
    }));
}

/// Hooked `gethostbyname` — legacy DNS resolution, same routing as getaddrinfo.
unsafe extern "system" fn hooked_gethostbyname(name: *const u8) -> *mut u8 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let Some(real) = REAL_GETHOSTBYNAME else {
            return std::ptr::null_mut();
        };
        if !name.is_null() {
            if let Ok(hostname) = CStr::from_ptr(name as *const i8).to_str() {
                match route_lookup("gethostbyname", hostname) {
                    Some(HostRoute::Redirect(svc)) => {
                        let marker = routing::marker_host_cstr(svc);
                        return real(marker.as_ptr());
                    }
                    Some(HostRoute::Block) => return std::ptr::null_mut(),
                    None => {}
                }
            }
        }
        real(name)
    }))
    .unwrap_or(std::ptr::null_mut())
}
