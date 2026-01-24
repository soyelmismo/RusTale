// Thanks to https://github.com/LiEnby/HytaleSP for the original C code

use std::ffi::c_void;
use std::fs::OpenOptions;
use std::io::Write;
use std::slice;
use std::sync::Mutex;

static LOG_FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);

fn init_logging() {
    if let Ok(dir) = std::env::var("RUSTALE_LOGS_DIR") {
        let path = std::path::Path::new(&dir).join("aurora.log");
        if let Ok(file) = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)
        {
            let mut lock = LOG_FILE.lock().unwrap();
            *lock = Some(file);
        }
    }
}

fn log_msg(msg: &str) {
    if let Ok(mut lock) = LOG_FILE.lock() {
        if let Some(file) = lock.as_mut() {
            let _ = writeln!(file, "{}", msg);
        }
    }
    println!("{}", msg);
}

macro_rules! log {
    ($($arg:tt)*) => {
        let msg = format!($($arg)*);
        crate::log_msg(&msg);
    }
}

// ==================== DATA TYPES ====================

// Hytale internal string structure: Length + Fixed Buffer
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
struct CsString {
    size: u32,
    data: [u16; 256],
}

impl CsString {
    fn from_str(s: &str) -> Self {
        let mut data = [0u16; 256];
        let mut len = 0;
        for (i, c) in s.encode_utf16().enumerate() {
            if i >= 255 {
                break;
            }
            data[i] = c;
            len = i + 1;
        }
        Self {
            size: len as u32,
            data,
        }
    }

    fn active_size_bytes(&self) -> usize {
        std::mem::size_of::<u32>() + (self.size as usize * std::mem::size_of::<u16>())
    }
}

// ==================== MEMORY PATCHING LOGIC ====================

unsafe fn raw_search_and_replace(mem: &mut [u8], target: &[u8], replacement: &[u8]) {
    if replacement.len() > target.len() {
        return;
    }
    let len = mem.len();
    let pat_len = target.len();

    if len < pat_len {
        return;
    }

    // Optimization: simple linear search
    for i in 0..=(len - pat_len) {
        if mem[i] != target[0] {
            continue;
        }

        if &mem[i..i + pat_len] == target {
            log!("Found raw pattern at offset: {}", i);
            let _guard = unsafe { MemoryProtectionGuard::new(mem.as_mut_ptr().add(i), pat_len) };

            for (j, &byte) in replacement.iter().enumerate() {
                mem[i + j] = byte;
            }
            // Null-pad the rest if replacement is shorter
            for j in replacement.len()..pat_len {
                mem[i + j] = 0;
            }
        }
    }
}

fn match_pattern(mem: &[u8], offset: usize) -> bool {
    if offset + 17 > mem.len() {
        return false;
    }

    #[cfg(target_os = "windows")]
    {
        // 48 8D ?? ?? ?? E8 ?? ?? ?? ?? 80 ?? ?? ?? 00 0F 84
        return mem[offset] == 0x48
            && mem[offset + 1] == 0x8D
            && mem[offset + 5] == 0xE8
            && mem[offset + 10] == 0x80
            && mem[offset + 14] == 0x00
            && mem[offset + 15] == 0x0F
            && mem[offset + 16] == 0x84;
    }
    #[cfg(target_os = "linux")]
    {
        // 48 8D ?? ?? E8 ?? ?? ?? 00 80 ?? ?? 00 0F 84
        return mem[offset] == 0x48
            && mem[offset + 1] == 0x8D
            && mem[offset + 4] == 0xE8
            && mem[offset + 8] == 0x00
            && mem[offset + 9] == 0x80
            && mem[offset + 12] == 0x00
            && mem[offset + 13] == 0x0F
            && mem[offset + 14] == 0x84;
    }
}

// ==================== PATCHING LOGIC ====================

unsafe fn allow_offline_in_online(addr: *mut u8, len: usize) {
    let slice = unsafe { slice::from_raw_parts_mut(addr, len) };

    for i in 0..len {
        if match_pattern(slice, i) {
            log!("Found offline/online pattern at offset {}", i);

            // Scan forward for JZ instructions (0x0F 0x84)
            // Logic adapted from C implementation: find first two JZ and NOP them
            let mut jz_found_count = 0;
            let mut current_offset = i;
            let max_scan = 500; // Safety limit
            let mut scanned = 0;

            while jz_found_count < 2 && scanned < max_scan && current_offset < len - 6 {
                if slice[current_offset] == 0x0F && slice[current_offset + 1] == 0x84 {
                    log!("Found JZ instruction (offset {})", current_offset);

                    // Enable writing for the NOP patch area
                    let _guard = unsafe { MemoryProtectionGuard::new(addr.add(current_offset), 6) };

                    // NOP out (0x90) x 6 bytes
                    for k in 0..6 {
                        slice[current_offset + k] = 0x90;
                    }
                    jz_found_count += 1;
                    current_offset += 6;
                } else {
                    current_offset += 1;
                }
                scanned += 1;
            }
            // Stop after first pattern match processed
            // (Assumes only one instance needs patching per region)
            if jz_found_count > 0 {
                break;
            }
        }
    }
}

unsafe fn patch_server_args(addr: *mut u8, len: usize) {
    let slice = unsafe { slice::from_raw_parts_mut(addr, len) };

    // Replace 'authenticated' with 'insecure' to bypass online mode checks
    let target_utf8 = b"authenticated";
    let replace_utf8 = b"insecure";

    let target_wide: Vec<u8> = "authenticated"
        .encode_utf16()
        .flat_map(|c| c.to_le_bytes())
        .collect();
    let replace_wide: Vec<u8> = "insecure"
        .encode_utf16()
        .flat_map(|c| c.to_le_bytes())
        .collect();

    unsafe { raw_search_and_replace(slice, target_utf8, replace_utf8) };
    unsafe { raw_search_and_replace(slice, &target_wide, &replace_wide) };
}

struct SwapEntry {
    old: CsString,
    new: CsString,
}

unsafe fn swap_strings(addr: *mut u8, len: usize) {
    let mode = std::env::var("AURORA_MODE").unwrap_or_else(|_| "local".to_string());
    let mut swaps = Vec::new();

    if mode == "sanasol" {
        swaps.push(SwapEntry {
            old: CsString::from_str("hytale.com"),
            new: CsString::from_str("sanasol.ws"),
        });
    } else {
        let port_str = std::env::var("AURORA_PORT").unwrap_or_else(|_| "59313".to_string());

        // Match logic from C implementation where possible
        let subdomains = vec![
            ("account-data", "000000000"),
            ("sessions", "00000"),
            ("telemetry", "000000"),
            ("tools", "00"),
        ];

        // Some versions of the binary might hold short strings (as seen in C patch)
        // or full strings. We attempt to match the C patch strategy exactly for robustness.

        // Strategy A: Full replacements (Go Launcher logic)
        for (subdomain, filler) in &subdomains {
            swaps.push(SwapEntry {
                old: CsString::from_str(&format!("https://{}.hytale.com", subdomain)),
                new: CsString::from_str(&format!("http://127.0.0.{}:{}", filler, port_str)),
            });
        }

        // Strategy B: Short replacements (C Patch logic) - In case binary uses split strings
        for (subdomain, filler) in &subdomains {
            swaps.push(SwapEntry {
                old: CsString::from_str(&format!("https://{}.", subdomain)),
                new: CsString::from_str(&format!("http://127.0.0.{}", filler)),
            });
        }

        swaps.push(SwapEntry {
            old: CsString::from_str("authenticated"),
            new: CsString::from_str("insecure"),
        });

        swaps.push(SwapEntry {
            old: CsString::from_str("hytale.com"),
            new: CsString::from_str(&format!("1:{}", port_str)),
        });

        // C-patch specific: .1:port vs hytale.com (len 8 vs 10)
        swaps.push(SwapEntry {
            old: CsString::from_str("hytale.com"),
            new: CsString::from_str(&format!(".1:{}", port_str)),
        });
    }

    let mut swaps_done = 0;

    for i in 0..len {
        for swap in &swaps {
            let active_size = swap.old.active_size_bytes();
            if i + active_size > len {
                continue;
            }

            // unsafe block to read raw memory for comparison
            let matches = unsafe {
                let mem_ptr = addr.add(i) as *const u8;
                let old_ptr = &swap.old as *const CsString as *const u8;
                let mut m = true;
                for j in 0..active_size {
                    if *mem_ptr.add(j) != *old_ptr.add(j) {
                        m = false;
                        break;
                    }
                }
                m
            };

            if matches {
                log!("Swapping string at offset {}", i);
                let _guard = unsafe { MemoryProtectionGuard::new(addr.add(i), active_size) };
                let new_ptr = &swap.new as *const CsString as *const u8;
                let copy_size = swap.new.active_size_bytes();

                unsafe {
                    std::ptr::copy_nonoverlapping(new_ptr, addr.add(i), copy_size);
                }
                swaps_done += 1;
            }
        }
    }
    if swaps_done > 0 {
        log!("Total string replacements in region: {}", swaps_done);
    }
}

// ==================== SYSTEM HELPERS ====================

struct MemoryInfo {
    start: *mut u8,
    size: usize,
}

struct MemoryProtectionGuard {
    addr: *mut c_void,
    size: usize,
    #[cfg(target_os = "windows")]
    old_protect: u32,
    #[cfg(target_os = "linux")]
    old_protect: i32,
}

#[cfg(target_os = "windows")]
unsafe fn get_memory_regions() -> Vec<MemoryInfo> {
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleA;
    use windows_sys::Win32::System::ProcessStatus::K32GetModuleInformation;
    use windows_sys::Win32::System::ProcessStatus::MODULEINFO;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    let h_module = unsafe { GetModuleHandleA(std::ptr::null()) };
    let mut info: MODULEINFO = unsafe { std::mem::zeroed() };
    unsafe {
        K32GetModuleInformation(
            GetCurrentProcess(),
            h_module,
            &mut info,
            std::mem::size_of::<MODULEINFO>() as u32,
        );
    }

    vec![MemoryInfo {
        start: info.lpBaseOfDll as *mut u8,
        size: info.SizeOfImage as usize,
    }]
}

#[cfg(target_os = "linux")]
unsafe fn get_memory_regions() -> Vec<MemoryInfo> {
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    let exe_path = std::fs::read_link("/proc/self/exe").unwrap_or_default();
    let exe_str = exe_path.to_string_lossy();

    let mut regions = Vec::new();

    if let Ok(file) = File::open("/proc/self/maps") {
        let reader = BufReader::new(file);

        for line in reader.lines() {
            if let Ok(l) = line {
                if l.contains(&*exe_str) {
                    // Parse line: 55a...-55a... rw-p ...
                    let parts: Vec<&str> = l.split_whitespace().collect();
                    if parts.len() < 2 {
                        continue;
                    }

                    let range_str = parts[0];
                    let perms = parts[1];

                    // We only care about Readable segments.
                    // To be safe and match C logic, we generally want segments that are
                    // executable (for code patch) or writable/readable (for data patch).
                    if !perms.contains('r') {
                        continue;
                    }

                    let ranges: Vec<&str> = range_str.split('-').collect();
                    if ranges.len() != 2 {
                        continue;
                    }

                    let start = usize::from_str_radix(ranges[0], 16).unwrap_or(0);
                    let end = usize::from_str_radix(ranges[1], 16).unwrap_or(0);

                    if end > start {
                        regions.push(MemoryInfo {
                            start: start as *mut u8,
                            size: end - start,
                        });
                    }
                }
            }
        }
    }

    // Sort logic not strictly needed as maps are ordered, but good for sanity
    regions
}

impl MemoryProtectionGuard {
    #[cfg(target_os = "windows")]
    unsafe fn new(addr: *mut u8, size: usize) -> Self {
        use windows_sys::Win32::System::Memory::{PAGE_EXECUTE_READWRITE, VirtualProtect};
        let mut old_protect = 0;
        unsafe {
            VirtualProtect(
                addr as *const c_void,
                size,
                PAGE_EXECUTE_READWRITE,
                &mut old_protect,
            );
        }
        Self {
            addr: addr as *mut c_void,
            size,
            old_protect,
        }
    }

    #[cfg(target_os = "linux")]
    unsafe fn new(addr: *mut u8, size: usize) -> Self {
        use libc::{_SC_PAGESIZE, PROT_EXEC, PROT_READ, PROT_WRITE, mprotect, sysconf};
        let page_size = unsafe { sysconf(_SC_PAGESIZE) as usize };
        let addr_usize = addr as usize;
        let page_start = addr_usize - (addr_usize % page_size);
        let len = (addr_usize + size) - page_start;

        // Ensure we make the page strictly RWX to perform modifications
        unsafe {
            mprotect(
                page_start as *mut c_void,
                len,
                PROT_READ | PROT_WRITE | PROT_EXEC,
            );
        }

        Self {
            addr: page_start as *mut c_void,
            size: len,
            old_protect: (PROT_READ | PROT_EXEC) as i32, // Default restore to RX
        }
    }
}

impl Drop for MemoryProtectionGuard {
    fn drop(&mut self) {
        unsafe {
            #[cfg(target_os = "windows")]
            {
                use windows_sys::Win32::System::Memory::VirtualProtect;
                let mut dummy = 0;
                VirtualProtect(self.addr, self.size, self.old_protect, &mut dummy);
            }
            #[cfg(target_os = "linux")]
            {
                use libc::mprotect;
                mprotect(self.addr, self.size, self.old_protect);
            }
        }
    }
}
// ==================== ENTRY POINTS ====================

unsafe fn main_logic() {
    init_logging();
    log!("Aurora Patcher initialized.");

    let regions = unsafe { get_memory_regions() };
    log!("Found {} memory regions for patching.", regions.len());

    for region in regions {
        if region.start.is_null() || region.size == 0 {
            continue;
        }

        // Try all patches on all valid regions.
        // The pattern matchers will simply fail quickly if the data isn't there.
        unsafe { allow_offline_in_online(region.start, region.size) };
        unsafe { swap_strings(region.start, region.size) };
        unsafe { patch_server_args(region.start, region.size) };
    }
}

#[cfg(target_os = "windows")]
#[unsafe(no_mangle)]
#[allow(non_snake_case, unused_variables)]
pub unsafe extern "system" fn DllMain(
    h_module: windows_sys::Win32::Foundation::HMODULE,
    ul_reason_for_call: u32,
    lp_reserved: *mut c_void,
) -> i32 {
    use windows_sys::Win32::System::SystemServices::DLL_PROCESS_ATTACH;

    if ul_reason_for_call == DLL_PROCESS_ATTACH {
        unsafe { main_logic() };
    }
    1
}

#[cfg(target_os = "windows")]
#[unsafe(no_mangle)]
pub unsafe extern "system" fn GetUserNameExW(_nfmt: i32, name_buf: *mut u16, sz: *mut i32) -> i32 {
    unsafe {
        if !sz.is_null() {
            *sz = 0;
        }
        if !name_buf.is_null() {
            *name_buf = 0;
        }
    }
    0
}

#[cfg(target_os = "linux")]
#[ctor::ctor]
unsafe fn init() {
    unsafe {
        let _ = std::env::remove_var("LD_PRELOAD");
    }
    unsafe { main_logic() };
}
