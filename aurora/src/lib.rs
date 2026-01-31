// Thanks to https://github.com/LiEnby/HytaleSP for the original C code

use std::ffi::c_void;
use std::fs::OpenOptions;
use std::io::Write;
use std::slice;
use std::sync::Mutex;

#[cfg(target_os = "linux")]
use libc;

static LOG_FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);

fn init_logging() {
    // Shared logging logic
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

struct SwapEntry {
    old: CsString,
    new: CsString,
}

struct MemoryInfo {
    start: *mut u8,
    size: usize,
    #[cfg(target_os = "linux")]
    prot: i32, // Store original protection flags for Linux
}

struct MemoryProtectionGuard {
    addr: *mut c_void,
    size: usize,
    #[cfg(target_os = "windows")]
    old_protect: u32,
    #[cfg(target_os = "linux")]
    restore_prot: i32,
}

// ==================== MEMORY PATCHING LOGIC ====================

fn match_pattern(mem: &[u8], offset: usize) -> bool {
    if offset + 17 > mem.len() {
        return false;
    }

    #[cfg(target_os = "windows")]
    {
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

unsafe fn allow_offline_in_online(region: &MemoryInfo) {
    let slice = unsafe { slice::from_raw_parts_mut(region.start, region.size) };
    let len = region.size;

    for i in 0..len {
        if match_pattern(slice, i) {
            log!("Found offline/online pattern at offset {}", i);

            let mut jz_found_count = 0;
            let mut current_offset = i;
            let max_scan = 500;
            let mut scanned = 0;

            while jz_found_count < 2 && scanned < max_scan && current_offset < len - 6 {
                if slice[current_offset] == 0x0F && slice[current_offset + 1] == 0x84 {
                    log!("Found JZ instruction (offset {})", current_offset);

                    // Pass region permissions (Linux) or ignore (Windows uses internal logic)
                    let _guard = unsafe {
                        MemoryProtectionGuard::new(
                            region.start.add(current_offset),
                            6,
                            #[cfg(target_os = "linux")]
                            region.prot,
                        )
                    };

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
            if jz_found_count > 0 {
                break;
            }
        }
    }
}

// ==================== STRINGS LOGIC (WINDOWS) ====================

#[cfg(target_os = "windows")]
unsafe fn raw_search_and_replace(mem: &mut [u8], target: &[u8], replacement: &[u8]) {
    if replacement.len() > target.len() {
        return;
    }
    let len = mem.len();
    let pat_len = target.len();
    if len < pat_len {
        return;
    }

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
            for j in replacement.len()..pat_len {
                mem[i + j] = 0;
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn get_swaps(mode: &str) -> Vec<SwapEntry> {
    let mut swaps = Vec::new();

    if mode == "sanasol" {
        swaps.push(SwapEntry {
            old: CsString::from_str("hytale.com"),
            new: CsString::from_str("sanasol.ws"),
        });
    } else {
        let port_str = std::env::var("AURORA_PORT").unwrap_or_else(|_| "59313".to_string());
        let subdomains = vec![
            ("account-data", "000000000"),
            ("sessions", "00000"),
            ("telemetry", "000000"),
            ("tools", "00"),
        ];

        for (subdomain, filler) in &subdomains {
            swaps.push(SwapEntry {
                old: CsString::from_str(&format!("https://{}.hytale.com", subdomain)),
                new: CsString::from_str(&format!("http://127.0.0.{}:{}", filler, port_str)),
            });
        }

        for (subdomain, filler) in &subdomains {
            let old_str = format!("https://{}.", subdomain);
            let old = CsString::from_str(&old_str);
            let new = CsString::from_str(&format!("http://127.0.0.{}", filler));
            swaps.push(SwapEntry { old, new });
        }

        swaps.push(SwapEntry {
            old: CsString::from_str("hytale.com"),
            new: CsString::from_str(&format!("1:{}", port_str)),
        });
        swaps.push(SwapEntry {
            old: CsString::from_str("hytale.com"),
            new: CsString::from_str(&format!(".1:{}", port_str)),
        });
    }
    swaps
}
#[cfg(target_os = "linux")]
fn get_dynamic_port() -> String {
    // Aurora lee directamente de la carpeta de runtime del usuario (SEAMLESS SYNC)
    // Prioridad 1: XDG_RUNTIME_DIR (estándar Linux)
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        let path = format!("{}/rustale/auth.port", runtime_dir);
        if let Ok(port) = std::fs::read_to_string(&path) {
            log!("[RusTale] Found dynamic port from XDG_RUNTIME_DIR: {}", port.trim());
            return port.trim().to_string();
        }
    }
    
    // Prioridad 2: Construcción manual usando UID (fallback robusto)
    let uid = unsafe { libc::getuid() };
    let fallback_path = format!("/run/user/{}/rustale/auth.port", uid);
    if let Ok(port) = std::fs::read_to_string(&fallback_path) {
        log!("[RusTale] Found dynamic port from fallback path: {}", port.trim());
        return port.trim().to_string();
    }
    
    // Prioridad 3: Variable de entorno (redundancia)
    if let Ok(port) = std::env::var("AURORA_PORT") {
        log!("[RusTale] Using AURORA_PORT env var: {}", port);
        return port;
    }
    
    // Default final
    log!("[RusTale] No port found, using default 59313");
    "59313".to_string()
}

#[cfg(target_os = "linux")]
fn get_swaps(mode: &str) -> Vec<SwapEntry> {
    let mut swaps = Vec::new();
    let port_str = get_dynamic_port(); // Puerto obtenido dinámicamente en cada inyección

    if mode == "sanasol" {
        swaps.push(SwapEntry {
            old: CsString::from_str("hytale.com"),
            new: CsString::from_str("sanasol.ws"),
        });
        // Agregar aqui los mismos dominios que Windows si sanasol.ws usa subdominios estandar
    } else {
        // --- LOGICA ACTUALIZADA CON PUERTO DINÁMICO ---
        // Usamos IPs falsas pero dentro de loopback 127.0.x.x para que el firewall 
        // piense que es comunicación interna de kernel.
        swaps.push(SwapEntry {
            old: CsString::from_str("https://account-data."),
            new: CsString::from_str("http://127.0.0.000000"),
        });

        swaps.push(SwapEntry {
            old: CsString::from_str("https://sessions."),
            new: CsString::from_str("http://127.0.0.00"),
        });

        swaps.push(SwapEntry {
            old: CsString::from_str("https://telemetry."),
            new: CsString::from_str("http://127.0.0.000"),
        });

        swaps.push(SwapEntry {
            old: CsString::from_str("https://tools."),
            new: CsString::from_str("http://127.000"),
        });

        swaps.push(SwapEntry {
            old: CsString::from_str("hytale.com"),
            new: CsString::from_str(&format!("0001:{}", port_str)),
        });
    }

    swaps
}
unsafe fn debug_read_csstr(ptr: *const u8) -> String {
    // 1. Read size
    let size = unsafe { std::ptr::read_unaligned(ptr as *const u32) };

    // Sanity check: if the size is absurd, return error
    if size > 512 {
        return "<invalid size>".to_string();
    }

    let data_ptr = unsafe { ptr.add(4) };
    let mut buf: Vec<u16> = Vec::with_capacity(size as usize);

    for k in 0..size {
        let char_ptr = unsafe { data_ptr.add((k as usize) * 2) as *const u16 };
        let c = unsafe { std::ptr::read_unaligned(char_ptr) };

        // critical: stop reading if we find a null terminator
        // or if the character seems garbage (optional, but helps clean logs)
        if c == 0 {
            break;
        }
        buf.push(c);
    }

    String::from_utf16_lossy(&buf)
}

// ==================== STRINGS LOGIC ====================

unsafe fn swap_strings(region: &MemoryInfo) {
    let addr = region.start;
    let len = region.size;
    let mode = std::env::var("AURORA_MODE").unwrap_or_else(|_| "local".to_string());

    let swaps = get_swaps(&mode);

    unsafe {
        apply_swaps(
            addr,
            len,
            &swaps,
            #[cfg(target_os = "linux")]
            region.prot,
        );
    }
}

unsafe fn apply_swaps(
    addr: *mut u8,
    len: usize,
    swaps: &[SwapEntry],
    #[cfg(target_os = "linux")] prot: i32,
) {
    #[cfg(target_os = "windows")]
    unsafe {
        internal_apply_swaps_windows(addr, len, swaps)
    };

    #[cfg(target_os = "linux")]
    unsafe {
        internal_apply_swaps_linux(addr, len, swaps, prot)
    };
}

#[cfg(target_os = "windows")]
unsafe fn internal_apply_swaps_windows(addr: *mut u8, len: usize, swaps: &[SwapEntry]) {
    let mut swaps_done = 0;
    for i in 0..len {
        for swap in swaps {
            let active_size_old = swap.old.active_size_bytes();
            if i + active_size_old > len {
                continue;
            }

            let matches = unsafe {
                let mem_ptr = addr.add(i) as *const u8;
                let old_ptr = &swap.old as *const CsString as *const u8;
                let mut m = true;
                for j in 0..active_size_old {
                    if *mem_ptr.add(j) != *old_ptr.add(j) {
                        m = false;
                        break;
                    }
                }
                m
            };

            if matches {
                let old_str_debug = unsafe { debug_read_csstr(addr.add(i)) };
                let new_slice = unsafe {
                    std::slice::from_raw_parts(
                        std::ptr::addr_of!(swap.new.data) as *const u16,
                        swap.new.size as usize,
                    )
                };
                let new_str_debug = String::from_utf16_lossy(new_slice);

                log!(
                    "Swapping at offset {}: '{}' -> '{}'",
                    i,
                    old_str_debug,
                    new_str_debug
                );
                let _guard = unsafe { MemoryProtectionGuard::new(addr.add(i), active_size_old) };
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

// New corrected logic for Linux
#[cfg(target_os = "linux")]
unsafe fn internal_apply_swaps_linux(addr: *mut u8, len: usize, swaps: &[SwapEntry], prot: i32) {
    let mut swaps_done = 0;
    for i in 0..len {
        for swap in swaps {
            let active_size_old = swap.old.active_size_bytes();
            if i + active_size_old > len {
                continue;
            }

            // FIX: Compare size - 1 (ignoring last byte/byte high) to replicate C behavior
            let compare_len = active_size_old - 1;

            let matches = unsafe {
                let mem_ptr = addr.add(i) as *const u8;
                let old_ptr = &swap.old as *const CsString as *const u8;
                let mut m = true;
                for j in 0..compare_len {
                    if *mem_ptr.add(j) != *old_ptr.add(j) {
                        m = false;
                        break;
                    }
                }
                m
            };

            if matches {
                let old_str_debug = unsafe { debug_read_csstr(addr.add(i)) };

                // For the "new", we read directly from the swap structure
                let new_slice = unsafe {
                    std::slice::from_raw_parts(
                        std::ptr::addr_of!(swap.new.data) as *const u16,
                        swap.new.size as usize,
                    )
                };

                let new_str_debug = String::from_utf16_lossy(new_slice);

                log!(
                    "Swapping at offset {}: '{}' -> '{}'",
                    i,
                    old_str_debug,
                    new_str_debug
                );
                let _guard =
                    unsafe { MemoryProtectionGuard::new(addr.add(i), active_size_old, prot) };
                let new_ptr = &swap.new as *const CsString as *const u8;

                // FIX: Copiar size - 1 para replicar C
                let copy_size = swap.new.active_size_bytes() - 1;

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

// ==================== SYSTEM INTERNALS ====================

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
    unsafe fn new(addr: *mut u8, size: usize, original_prot: i32) -> Self {
        use libc::{_SC_PAGESIZE, PROT_EXEC, PROT_READ, PROT_WRITE, mprotect, sysconf};
        let page_size = unsafe { sysconf(_SC_PAGESIZE) as usize };
        let addr_usize = addr as usize;
        let page_start = addr_usize - (addr_usize % page_size);
        let len = (addr_usize + size) - page_start;

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
            restore_prot: original_prot,
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
                mprotect(self.addr, self.size, self.restore_prot);
            }
        }
    }
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
    use libc::{PROT_EXEC, PROT_READ, PROT_WRITE};
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
                    let parts: Vec<&str> = l.split_whitespace().collect();
                    if parts.len() < 2 {
                        continue;
                    }

                    let range_str = parts[0];
                    let perms = parts[1];

                    // Parse permissions for correct restoration later
                    let mut prot = 0;
                    if perms.contains('r') {
                        prot |= PROT_READ;
                    }
                    if perms.contains('w') {
                        prot |= PROT_WRITE;
                    }
                    if perms.contains('x') {
                        prot |= PROT_EXEC;
                    }

                    if prot & PROT_READ == 0 {
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
                            prot,
                        });
                    }
                }
            }
        }
    }
    regions
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

        unsafe { allow_offline_in_online(&region) };
        unsafe { swap_strings(&region) };
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
