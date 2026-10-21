use anyhow::{bail, Context, Result};
use libloading::{Library, Symbol};
use std::path::Path;
use std::sync::OnceLock;

type OodleLZDecompressFn = unsafe extern "C" fn(
    src_buf: *const u8,
    src_len: i64,
    dst_buf: *mut u8,
    dst_len: i64,
    fuzz_safe: i32,
    check_crc: i32,
    verbosity: i32,
    dst_base: *const u8,
    dst_base_len: i64,
    callback: *const u8,
    callback_ctx: *const u8,
    scratch: *mut u8,
    scratch_size: i64,
    thread_phase: i32,
) -> i64;

type OodleLZCompressFn = unsafe extern "C" fn(
    codec: i32,
    src_buf: *const u8,
    src_len: i64,
    dst_buf: *mut u8,
    level: i32,
    opts: *const u8,
    dst_base: *const u8,
    lrm: *const u8,
    scratch: *mut u8,
    scratch_size: i64,
) -> i64;

static OODLE_LIB: OnceLock<Library> = OnceLock::new();

#[repr(i32)]
#[derive(Debug, Clone, Copy)]
pub enum OodleCompressor {
    Kraken = 8,
    Mermaid = 9,
    Selkie = 11,
    Leviathan = 13,
}

#[repr(i32)]
#[derive(Debug, Clone, Copy)]
pub enum OodleCompressionLevel {
    None = 0,
    SuperFast = 1,
    VeryFast = 2,
    Fast = 3,
    Normal = 4,
    Optimal1 = 5,
    Optimal2 = 6,
    Optimal3 = 7,
    Optimal4 = 8,
    Optimal5 = 9,
}

fn load_library(dll_path: &Path) -> Result<&'static Library> {
    if let Some(lib) = OODLE_LIB.get() {
        return Ok(lib);
    }
    let lib = unsafe {
        Library::new(dll_path).with_context(|| {
            format!(
                "Failed to load Oodle DLL from {}. Make sure the game is installed.",
                dll_path.display()
            )
        })?
    };
    Ok(OODLE_LIB.get_or_init(|| lib))
}

pub fn init(dll_path: &Path) -> Result<()> {
    load_library(dll_path)?;
    Ok(())
}

pub fn decompress(dll_path: &Path, src: &[u8], dst_len: usize) -> Result<Vec<u8>> {
    let lib = load_library(dll_path)?;

    let decompress_fn: Symbol<OodleLZDecompressFn> = unsafe {
        lib.get(b"OodleLZ_Decompress")
            .context("Failed to find OodleLZ_Decompress in Oodle DLL")?
    };

    let mut dst = vec![0u8; dst_len];

    let result = unsafe {
        decompress_fn(
            src.as_ptr(),
            src.len() as i64,
            dst.as_mut_ptr(),
            dst_len as i64,
            1, // fuzz_safe = yes
            0, // check_crc = no
            0, // verbosity = none
            std::ptr::null(),
            0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            0,
            0, // thread_phase
        )
    };

    if result <= 0 {
        bail!(
            "Oodle decompression failed (returned {}). src_len={}, expected_dst_len={}",
            result,
            src.len(),
            dst_len
        );
    }

    dst.truncate(result as usize);
    Ok(dst)
}

pub fn compress(
    dll_path: &Path,
    src: &[u8],
    compressor: OodleCompressor,
    level: OodleCompressionLevel,
) -> Result<Vec<u8>> {
    let lib = load_library(dll_path)?;

    let compress_fn: Symbol<OodleLZCompressFn> = unsafe {
        lib.get(b"OodleLZ_Compress")
            .context("Failed to find OodleLZ_Compress in Oodle DLL")?
    };

    let bound = src.len() + (src.len() / 4) + 0x40000;
    let mut dst = vec![0u8; bound];

    let result = unsafe {
        compress_fn(
            compressor as i32,
            src.as_ptr(),
            src.len() as i64,
            dst.as_mut_ptr(),
            level as i32,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            0,
        )
    };

    if result <= 0 {
        bail!(
            "Oodle compression failed (returned {}). src_len={}",
            result,
            src.len()
        );
    }

    dst.truncate(result as usize);
    Ok(dst)
}
