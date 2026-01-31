use std::ffi::c_void;
use std::slice;
use std::sync::Mutex;
use std::fs::OpenOptions;
use std::io::Write;

#[cfg(target_os = "linux")]
use libc;

const DEFAULT_PORT: u16 = 59313;

// ==================== LOGGING SYSTEM ====================

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

// ==================== ESTRUCTURAS DE DATOS ====================

// Estructura en memoria de las Strings de Hytale (Length-Prefixed UTF16)
// Corresponde a 'csString' del código original en C.
#[repr(C, packed)]
struct CsStringHeader {
    size: u32,
    // data sigue inmediatamente después
}

struct SwapDefinition {
    pattern_bytes: Vec<u8>,
    replacement_bytes: Vec<u8>,
}

impl SwapDefinition {
    fn new(original: &str, replacement: &str) -> Self {
        Self {
            pattern_bytes: encode_cs_string_bytes(original),
            replacement_bytes: encode_cs_string_bytes(replacement),
        }
    }
}

// Codifica un str a formato binario [Length: u32][Data: utf16... no null term]
fn encode_cs_string_bytes(s: &str) -> Vec<u8> {
    let utf16: Vec<u16> = s.encode_utf16().collect();
    let size = utf16.len() as u32;
    
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&size.to_le_bytes()); // Header
    for c in utf16 {
        bytes.extend_from_slice(&c.to_le_bytes()); // Data
    }
    bytes
}

// ==================== LISTA DE PARCHES ====================

fn get_swaps() -> Vec<SwapDefinition> {
    let mut swaps = Vec::new();
    let port = std::env::var("AURORA_PORT").unwrap_or_else(|_| DEFAULT_PORT.to_string());
    
    // NOTA TÉCNICA (IMPORTANTE PARA LINUX):
    // La memoria en el binario compilado está empaquetada estrictamente.
    // Reemplazar una cadena con una de diferente longitud CORROMPE los datos adyacentes.
    // Se usan ceros '0' en las IPs para hacer padding hasta alcanzar la longitud exacta de la string original.
    // IP Objetivo Lógica: 127.0.0.1
    // La resolución de IPs estándar ignora ceros a la izquierda en octetos (ej. 0001 = 1).

    // --- Parte 1: Sufijo de Dominio ---
    // Original: "hytale.com" (10 chars)
    // Nuevo:    "0001:59313" (10 chars)
    // Efecto: Cuando se concatena con el prefijo, termina en ...001:59313
    swaps.push(SwapDefinition::new(
        "hytale.com",
        &format!("0001:{}", port) 
    ));

    // --- Parte 2: Prefijos (padding exacto) ---
    
    // 1. "https://account-data." (21 chars) -> "http://127.000.000.00" (21 chars)
    // Resultado final: http://127.000.000.000001:59313 (Valid IP)
    swaps.push(SwapDefinition::new(
        "https://account-data.",
        "http://127.0.0.000000"
    ));

    // 2. "https://sessions." (17 chars) -> "http://127.0.0.000" (17 chars)
    // Resultado final: http://127.0.0.0000001:59313
    swaps.push(SwapDefinition::new(
        "https://sessions.",
        "http://127.0.0.00"
    ));

    // 3. "https://telemetry." (18 chars) -> "http://127.0.0.0000" (18 chars)
    // Resultado final: http://127.0.0.00000001:59313
    swaps.push(SwapDefinition::new(
        "https://telemetry.",
        "http://127.0.0.000"
    ));

    // 4. "https://tools." (14 chars) -> "http://127.0.0" (14 chars)
    // Resultado final: http://127.0.00001:59313
    swaps.push(SwapDefinition::new(
        "https://tools.",
        "http://127.0.0"
    ));
    
    // 5. Argumentos CLI: Reemplazos directos
    // Original: "--session-token" (15 chars) -> "--singleplayer " (15 chars)
    // Nota: Padding con espacio al final para mantener length
    // Nota C original reemplazaba --session-token por --singleplayer pero el resto se quedaba sucio.
    // Aquí es más limpio padding con espacio o nulls (espacio es seguro en args).
    // Usaremos un string filler seguro.
    
    // Para argumentos específicos, usamos la lógica de anulación.
    // C code used: .new = make_csstr(L"--singleplayer=\"") (15 chars?? No)
    // C: "--session-token=\"" (17 chars). New: "--singleplayer=\"" (16 chars + ??)
    // Vamos a usar reemplazo estricto también.
    swaps.push(SwapDefinition::new("authenticated", "insecure\0\0\0\0")); // 13 -> 8 + padding nulls (C# ignora después de null? No, depende, pero 'insecure' es válido)
    
    // Fix especial session token
    // Original: --session-token
    // Nuevo:    --singleplayer 
    swaps.push(SwapDefinition::new("--session-token", "--singleplayer ")); 
    swaps.push(SwapDefinition::new("--identity-token", "--singleplayer "));

    swaps
}

// ==================== LÓGICA DE MEMORIA (System Agnostic Logic) ====================

struct MemoryRegion {
    addr: *mut u8,
    size: usize,
    #[cfg(target_os = "linux")]
    prot: i32, // Para restaurar protección en Linux
}

// Gestiona los permisos rwx temporalmente
struct ScopedProtect {
    addr: *mut c_void,
    size: usize,
    #[cfg(target_os = "windows")]
    old_protect: u32,
    #[cfg(target_os = "linux")]
    orig_prot: i32,
}

impl ScopedProtect {
    #[cfg(target_os = "linux")]
    unsafe fn new(addr: *mut u8, len: usize, region_prot: i32) -> Self {
        use libc::{mprotect, PROT_READ, PROT_WRITE, PROT_EXEC, sysconf, _SC_PAGESIZE};
        
        let page_size = sysconf(_SC_PAGESIZE) as usize;
        let addr_usize = addr as usize;
        let page_start = addr_usize - (addr_usize % page_size);
        let protect_len = (addr_usize + len) - page_start + page_size; // Ensure covering logic
        
        mprotect(page_start as *mut c_void, protect_len, PROT_READ | PROT_WRITE | PROT_EXEC);
        
        Self { addr: page_start as *mut c_void, size: protect_len, orig_prot: region_prot }
    }
    
    #[cfg(target_os = "windows")]
    unsafe fn new(addr: *mut u8, len: usize) -> Self {
        use windows_sys::Win32::System::Memory::{VirtualProtect, PAGE_EXECUTE_READWRITE};
        let mut old = 0;
        unsafe { VirtualProtect(addr as *mut _, len, PAGE_EXECUTE_READWRITE, &mut old); }
        Self { addr: addr as *mut _, size: len, old_protect: old }
    }
}

impl Drop for ScopedProtect {
    fn drop(&mut self) {
        unsafe {
            #[cfg(target_os = "linux")]
            libc::mprotect(self.addr, self.size, self.orig_prot);
            
            #[cfg(target_os = "windows")]
            {
                let mut dummy = 0;
                windows_sys::Win32::System::Memory::VirtualProtect(self.addr, self.size, self.old_protect, &mut dummy);
            }
        }
    }
}

// Bypass de Singleplayer Check
// Busca la secuencia JZ (Jump if Zero) que verifica singleplayer/auth mode y la reemplaza por NOPs.
unsafe fn patch_offline_check(region: &MemoryRegion) {
    log!("[Aurora] Iniciando búsqueda de checks de singleplayer");
    
    let slice = unsafe { slice::from_raw_parts(region.addr, region.size) };
    let mut i = 0;
    // Límite de seguridad
    if region.size < 20 { 
        log!("[Aurora] Región de memoria demasiado pequeña para parcheo: {} bytes", region.size);
        return; 
    }

    let mut checks_patched = 0;

    while i < region.size - 17 {
        let is_match;
        
        #[cfg(target_os = "linux")]
        {
            // Linux Pattern: 48 8D ?? ?? E8 ?? ?? ?? 00 80 ?? ?? 00 0F 84
            is_match = slice[i] == 0x48 && slice[i+1] == 0x8D && slice[i+4] == 0xE8 
                      && slice[i+8] == 0x00 && slice[i+9] == 0x80 && slice[i+12] == 0x00
                      && slice[i+13] == 0x0F && slice[i+14] == 0x84;
        }

        #[cfg(target_os = "windows")]
        {
             // Windows Pattern: 48 8D ?? ?? ?? E8 ?? ?? ?? ?? 80 ?? ?? ?? 00 0F 84
             is_match = slice[i] == 0x48 && slice[i+1] == 0x8D && slice[i+5] == 0xE8
                       && slice[i+10] == 0x80 && slice[i+14] == 0x00
                       && slice[i+15] == 0x0F && slice[i+16] == 0x84;
        }

        if is_match {
            log!("[Aurora] Check de singleplayer encontrado en offset +{:X}", i);

            // Búsqueda de JZs cercanos para parchear (NOP = 0x90)
            // Se necesitan encontrar 2 JZs.
            let mut cursor = i;
            let mut jz_found = 0;
            
            // Permitimos acceso RW
            #[cfg(target_os = "linux")]
            let _guard = ScopedProtect::new(region.addr.add(i), 500, region.prot);
            #[cfg(target_os = "windows")]
            let _guard = unsafe { ScopedProtect::new(region.addr.add(i), 500) };

            let writable_slice = unsafe { slice::from_raw_parts_mut(region.addr, region.size) };

            while jz_found < 2 && cursor < region.size - 6 && cursor < i + 500 {
                 if writable_slice[cursor] == 0x0F && writable_slice[cursor+1] == 0x84 {
                     // Rellenar con NOPs (0x90) los 6 bytes de la instrucción (JZ suele ser largo en x64 si salta lejos, pero verificamos opcode 0F 84 standard rel32)
                     // Normalmente 0F 84 XX XX XX XX (6 bytes).
                     for k in 0..6 {
                         writable_slice[cursor + k] = 0x90;
                     }
                     log!("[Aurora] JZ parcheado en offset +{:X}", cursor);
                     jz_found += 1;
                     cursor += 6; // saltar
                 } else {
                     cursor += 1;
                 }
            }
            
            if jz_found > 0 {
                checks_patched += 1;
                log!("[Aurora] {} JZs parcheados en este check", jz_found);
            }
        }
        i += 1;
    }
    
    log!("[Aurora] Búsqueda de checks completada: {} grupos de JZs parcheados", checks_patched);
}

// Intercambio de Strings
unsafe fn apply_swaps(region: &MemoryRegion) {
    init_logging();
    log!("[Aurora] Iniciando búsqueda y reemplazo de strings");
    let mut swaps = Vec::new();
    let mode = std::env::var("AURORA_MODE").unwrap_or_else(|_| "local".to_string());
    
    if mode == "sanasol" {
        swaps.push(SwapDefinition::new(
            "hytale.com.",
            "sanasol.ws"
        ));
        // Agregar aqui los mismos dominios que Windows si sanasol.ws usa subdominios estandar
    } else {
        swaps = get_swaps()
    }
    let mem = unsafe { slice::from_raw_parts(region.addr, region.size) };
    let mut replacements_found = 0;
    let mut replacements_applied = 0;
    
    // Itera sobre la memoria buscando headers de strings
    for i in 0..region.size {
        // Optimización rápida: verifica si podría ser un string (longitud posible?)
        if i + 4 >= region.size { break; }
        
        // El primer u32 es la longitud en caracteres.
        // Hytale (Mono/C#) strings struct: [u32 Length] [u16 Char] ...
        // No leemos la longitud directamente para saltar, buscamos byte-a-byte patrones conocidos.
        
        for swap in &swaps {
            let pattern = &swap.pattern_bytes;
            if i + pattern.len() > region.size { continue; }

            // IMPORTANTE: Reproducir la lógica del código C original (get_size_ptr)
            // El código C hacía memcmp de: 4 + (2*Len) - 1.
            // Es decir, ignoraba el ÚLTIMO byte del string en la comparación.
            // Esto permite "matchear" incluso si hay basura o un null terminator raro en el último byte high.
            
            let compare_len = pattern.len() - 1; // Magic trick de C para Linux matching
            
            if &mem[i..i+compare_len] == &pattern[0..compare_len] {
                replacements_found += 1;
                
                // Extraer el string original para logging
                let original_str = if i + 4 <= region.size {
                    let len_bytes = &mem[i..i+4];
                    let len = u32::from_le_bytes([len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3]]) as usize;
                    if i + 4 + len * 2 <= region.size {
                        let utf16_bytes = &mem[i+4..i+4+len*2];
                        let mut utf16_vec = Vec::new();
                        for chunk in utf16_bytes.chunks_exact(2) {
                            utf16_vec.push(u16::from_le_bytes([chunk[0], chunk[1]]));
                        }
                        String::from_utf16_lossy(&utf16_vec)
                    } else {
                        "<invalid length>".to_string()
                    }
                } else {
                    "<invalid header>".to_string()
                };
                
                log!("[Aurora] String encontrado en {:p}: '{}' -> reemplazando", unsafe { region.addr.add(i) }, original_str);

                // Abrir candado de memoria
                #[cfg(target_os = "linux")]
                let _guard = ScopedProtect::new(region.addr.add(i), swap.replacement_bytes.len(), region.prot);
                #[cfg(target_os = "windows")]
                let _guard = unsafe { ScopedProtect::new(region.addr.add(i), swap.replacement_bytes.len()) };
                
                let writable_mem = unsafe { slice::from_raw_parts_mut(region.addr, region.size) };
                
                // Sobrescribir exactamente la longitud del pattern
                // Si la replacement es más corta, se ha hecho padding o se rellenan con 0 si así se definiera (en este caso usamos padding estricto)
                let copy_len = swap.replacement_bytes.len().min(pattern.len()); // Seguridad
                
                // NOTA: Rust copy_from_slice es seguro, pero necesitamos escritura raw punteros en memoria protegida (aunque ScopedProtect ya la abrió)
                writable_mem[i..i+copy_len].copy_from_slice(&swap.replacement_bytes[0..copy_len]);
                
                replacements_applied += 1;
                log!("[Aurora] Reemplazo aplicado exitosamente");
            }
        }
    }
    
    log!("[Aurora] Búsqueda completada: {} strings encontrados, {} reemplazos aplicados", replacements_found, replacements_applied);
}


// ==================== DISCOVERY MEMORIA ====================

#[cfg(target_os = "linux")]
unsafe fn scan_and_patch() {
    use std::io::{BufRead, BufReader};
    use std::fs::File;
    use libc::{PROT_READ, PROT_WRITE, PROT_EXEC};

    // Obtener path del exe actual para filtrar regiones
    let mut exe_path = [0u8; 1024];
    let len = libc::readlink(
        "/proc/self/exe\0".as_ptr() as *const i8, 
        exe_path.as_mut_ptr() as *mut i8, 
        1023
    );
    if len <= 0 { return; }
    let exe_name = std::str::from_utf8(&exe_path[0..len as usize]).unwrap_or("");

    if let Ok(f) = File::open("/proc/self/maps") {
        let reader = BufReader::new(f);
        for line in reader.lines() {
            if let Ok(l) = line {
                if !l.contains(exe_name) { continue; }
                
                // Parse: 00400000-00452000 r-xp ...
                let parts: Vec<&str> = l.split_whitespace().collect();
                if parts.len() < 2 { continue; }
                
                let range_parts: Vec<&str> = parts[0].split('-').collect();
                if range_parts.len() != 2 { continue; }
                
                let start = usize::from_str_radix(range_parts[0], 16).unwrap_or(0);
                let end = usize::from_str_radix(range_parts[1], 16).unwrap_or(0);
                let perms = parts[1];
                
                let mut prot = 0;
                if perms.contains('r') { prot |= PROT_READ; }
                if perms.contains('w') { prot |= PROT_WRITE; }
                if perms.contains('x') { prot |= PROT_EXEC; }

                if (prot & PROT_READ) != 0 {
                     let region = MemoryRegion { addr: start as *mut u8, size: end - start, prot };
                     patch_offline_check(&region);
                     apply_swaps(&region);
                }
            }
        }
    }
}

#[cfg(target_os = "windows")]
unsafe fn scan_and_patch() {
    use windows_sys::Win32::System::ProcessStatus::K32GetModuleInformation;
    use windows_sys::Win32::System::ProcessStatus::MODULEINFO;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleA;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    
    let h_mod = unsafe { GetModuleHandleA(std::ptr::null()) };
    let mut info: MODULEINFO = unsafe {std::mem::zeroed()};
    unsafe { K32GetModuleInformation(GetCurrentProcess(), h_mod, &mut info, std::mem::size_of::<MODULEINFO>() as u32) };
    
    let region = MemoryRegion { addr: info.lpBaseOfDll as *mut u8, size: info.SizeOfImage as usize };
    unsafe { patch_offline_check(&region) };
    unsafe { apply_swaps(&region) };
}

// ==================== ENTRY POINTS ====================

// Linux Constructor (se ejecuta al cargar la librería .so)
#[cfg(target_os = "linux")]
#[ctor::ctor]
unsafe fn aurora_init() {
    init_logging();
    log!("[Aurora] Inicializando Aurora para Linux");
    
    // Eliminar LD_PRELOAD para no afectar subprocesos
    std::env::remove_var("LD_PRELOAD");
    log!("[Aurora] LD_PRELOAD eliminado");
    
    scan_and_patch();
    log!("[Aurora] Inicialización completada");
}

// Windows DllMain
#[cfg(target_os = "windows")]
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllMain(
    _inst: isize,
    reason: u32,
    _reserved: *const c_void
) -> i32 {
    if reason == 1 { // DLL_PROCESS_ATTACH
        init_logging();
        log!("[Aurora] Inicializando Aurora para Windows");
        
        unsafe { scan_and_patch(); }
        log!("[Aurora] Inicialización completada");
    }
    1
}

// Export para Windows (stubs necesarios para que el juego cargue la DLL pensando que es Secur32)
#[cfg(target_os = "windows")]
#[unsafe(no_mangle)]
pub unsafe extern "system" fn GetUserNameExW(_format: i32, buffer: *mut u16, size: *mut u32) -> u8 {
    unsafe {
    if !size.is_null() { *size = 0; }
    if !buffer.is_null() { *buffer = 0; }
    }
    0
}
