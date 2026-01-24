use std::ffi::c_void;
use std::fs::OpenOptions;
use std::io::Write;
use std::slice;
use std::sync::Mutex;
static LOG_FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);

fn init_logging() {
    if let Ok(dir) = std::env::var("RUSTALE_LOGS_DIR") {
        let path = std::path::Path::new(&dir).join("aurora.log");
        // Truncate file on start
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
#[derive(Debug)]
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

// Define the search patterns according to the original C file
#[cfg(target_os = "linux")]
const PATTERN: [Option<u8>; 17] = [
    Some(0x48),
    Some(0x8D),
    None,
    None,
    Some(0xE8),
    None,
    None,
    None,
    Some(0x00),
    Some(0x80),
    None,
    None,
    Some(0x00),
    Some(0x0F),
    Some(0x84),
    None,
    None, // Adjusted lengths
];

unsafe fn raw_search_and_replace(mem: &mut [u8], target: &[u8], replacement: &[u8]) {
    log!("Searching for pattern: {:?}", target);
    log!("Replacement: {:?}", replacement);
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
            log!("Found pattern at offset: {}", i);
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
    log!("Allowing offline in online");
    let slice = unsafe {
        let slice = slice::from_raw_parts_mut(addr, len);
        slice
    };

    for i in 0..len {
        if match_pattern(slice, i) {
            log!("Found pattern at offset {}", i);

            // Enable writing
            let _guard = unsafe {
                log!("Enabling writing");
                let _guard = MemoryProtectionGuard::new(addr.add(i), 100);
                _guard
            };

            let mut jz_found_count = 0;
            let mut current_offset = i;

            while jz_found_count < 2 && current_offset < len - 1 {
                if slice[current_offset] == 0x0F && slice[current_offset + 1] == 0x84 {
                    log!("Found JZ instruction (offset {})", current_offset);
                    // NOP out (0x90) x 6 bytes
                    for k in 0..6 {
                        if current_offset + k < len {
                            slice[current_offset + k] = 0x90;
                        }
                    }
                    jz_found_count += 1;
                    current_offset += 6;
                } else {
                    current_offset += 1;
                }
            }
            break;
        }
    }
}

unsafe fn patch_server_args(addr: *mut u8, len: usize) {
    log!("Patching server launch arguments...");
    let slice = unsafe { slice::from_raw_parts_mut(addr, len) };

    // Replace 'authenticated' with 'insecure' to bypass online mode checks in the server
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
    log!("Swapping URLs...");
    let mode = std::env::var("AURORA_MODE").unwrap_or_else(|_| "local".to_string());
    let mut swaps = Vec::new();

    if mode == "sanasol" {
        log!("Mode is Sanasol");
        swaps.push(SwapEntry {
            old: CsString::from_str("hytale.com"),
            new: CsString::from_str("sanasol.ws"),
        });
    } else {
        log!("Mode is Local");

        let port_str = std::env::var("AURORA_PORT").unwrap_or_else(|_| "59313".to_string());
        log!("Target port suffix: {}", port_str);

        let subdomains = vec![
            ("account-data", "000000000"), // fills https://account-data.hytale.com -> http://127.0.0.0000000001:59313
            ("sessions", "00000"), // fills https://sessions.hytale.com -> http://127.0.0.000001:59313
            ("telemetry", "000000"), // fills https://telemetry.hytale.com -> http://127.0.0.0000001:59313
            ("tools", "00"),         // fills https://tools.hytale.com -> http://127.0.0.001:59313
        ];
        for (subdomain, filler) in subdomains {
            swaps.push(SwapEntry {
                // El original para buscar
                old: CsString::from_str(&format!("https://{}.", subdomain)),
                // El nuevo, limpio y sin caracteres basura
                new: CsString::from_str(&format!("http://127.0.0.{}", filler)),
            });
        }

        // authenticated to insecure
        swaps.push(SwapEntry {
            old: CsString::from_str("authenticated"),
            new: CsString::from_str("insecure"),
        });

        swaps.push(SwapEntry {
            old: CsString::from_str("hytale.com"),
            new: CsString::from_str(&format!("1:{}", port_str)),
        });
    }

    for sap in &swaps {
        log!("Swapping {:?} to {:?}", sap.old, sap.new);
        log!("Old length: {}", sap.old.active_size_bytes());
        log!("New length: {}", sap.new.active_size_bytes());
    }

    let total_swaps_needed = swaps.len();
    if total_swaps_needed == 0 {
        return;
    }
    log!("Swapping {} strings", total_swaps_needed);

    let mut swaps_done = 0;
    let slice = unsafe {
        let slice = slice::from_raw_parts_mut(addr, len);
        slice
    };

    for i in 0..len {
        if swaps_done >= total_swaps_needed {
            break;
        }

        for swap in &swaps {
            let active_size = swap.old.active_size_bytes();
            if i + active_size > len {
                continue;
            }

            let old_ptr = &swap.old as *const CsString as *const u8;
            let mem_ptr = &slice[i] as *const u8;
            let mut matches = true;
            for j in 0..active_size {
                if unsafe { *mem_ptr.add(j) != *old_ptr.add(j) } {
                    matches = false;
                    break;
                }
            }

            if matches {
                log!("Found match at offset {}", i);
                let _guard = unsafe {
                    let _guard = MemoryProtectionGuard::new(addr.add(i), active_size);
                    _guard
                };
                let new_ptr = &swap.new as *const CsString as *const u8;
                let copy_size = swap.new.active_size_bytes();

                unsafe {
                    std::ptr::copy_nonoverlapping(new_ptr, addr.add(i), copy_size);
                }

                swaps_done += 1;
            }
        }
    }
    log!("Total replacements: {}", swaps_done);
}

// ==================== SYSTEM HELPERS ====================

struct MemoryInfo {
    start: *mut u8,
    size: usize,
}

#[cfg(target_os = "windows")]
unsafe fn get_base_module() -> MemoryInfo {
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleA;
    use windows_sys::Win32::System::ProcessStatus::K32GetModuleInformation;
    use windows_sys::Win32::System::ProcessStatus::MODULEINFO;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    let h_module = unsafe {
        let h_module = GetModuleHandleA(std::ptr::null());
        h_module
    };
    let mut info: MODULEINFO = unsafe { std::mem::zeroed() };
    unsafe {
        K32GetModuleInformation(
            GetCurrentProcess(),
            h_module,
            &mut info,
            std::mem::size_of::<MODULEINFO>() as u32,
        );
    }

    MemoryInfo {
        start: info.lpBaseOfDll as *mut u8,
        size: info.SizeOfImage as usize,
    }
}

struct MemoryProtectionGuard {
    addr: *mut c_void,
    size: usize,
    #[cfg(target_os = "windows")]
    old_protect: u32,
    #[cfg(target_os = "linux")]
    old_protect: i32,
}

#[cfg(target_os = "linux")]
unsafe fn get_base_module() -> MemoryInfo {
    // Simplified implementation of reading /proc/self/maps like in the original C
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    let exe_path = std::fs::read_link("/proc/self/exe").unwrap_or_default();
    let exe_str = exe_path.to_string_lossy();

    let file = File::open("/proc/self/maps").expect("Cannot open maps");
    let reader = BufReader::new(file);

    let mut start: usize = 0;
    let mut end: usize = 0;
    let mut found = false;

    for line in reader.lines() {
        if let Ok(l) = line {
            if l.contains(&*exe_str) {
                let parts: Vec<&str> = l.split_whitespace().collect();
                if let Some(range) = parts.get(0) {
                    let ranges: Vec<&str> = range.split('-').collect();
                    let s = usize::from_str_radix(ranges[0], 16).unwrap_or(0);
                    let e = usize::from_str_radix(ranges[1], 16).unwrap_or(0);

                    if !found {
                        start = s;
                        found = true;
                    }
                    end = e;
                }
            }
            MemoryInfo {
                start: start as *mut u8,
                size: end - start,
            }
        }
    }
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
        use libc::{PROT_EXEC, PROT_READ, PROT_WRITE, getpagesize, mprotect};
        let page_size = getpagesize() as usize;
        let addr_usize = addr as usize;
        let page_start = addr_usize - (addr_usize % page_size);
        let len = (addr_usize + size) - page_start;

        mprotect(
            page_start as *mut c_void,
            len,
            PROT_READ | PROT_WRITE | PROT_EXEC,
        );

        Self {
            addr: page_start as *mut c_void,
            size: len,
            old_protect: (PROT_READ | PROT_EXEC) as i32,
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
    let mod_info = unsafe { get_base_module() };
    if mod_info.start.is_null() || mod_info.size == 0 {
        return;
    }

    unsafe { allow_offline_in_online(mod_info.start, mod_info.size) };
    unsafe { swap_strings(mod_info.start, mod_info.size) };
    unsafe { patch_server_args(mod_info.start, mod_info.size) };
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
        // Run logic
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
    // Cleanup
    let _ = std::env::remove_var("LD_PRELOAD");
    unsafe { main_logic() };
}
