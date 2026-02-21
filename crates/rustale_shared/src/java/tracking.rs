use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::time::SystemTime;

/// Estructura para tracking seguro de PIDs
#[derive(Debug, Clone)]
pub struct TrackedProcess {
    pub pid: u32,
    pub start_time: SystemTime,
    pub process_name: String,
    pub created_by_rustale: bool,
}

/// Gestor global de PIDs para tracking seguro
pub static PID_TRACKER: std::sync::LazyLock<Arc<Mutex<HashMap<u32, TrackedProcess>>>> = 
    std::sync::LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

/// Registra un proceso creado por RusTale
pub fn track_process(pid: u32, process_name: String) {
    if let Ok(mut tracker) = PID_TRACKER.lock() {
        let process = TrackedProcess {
            pid,
            start_time: SystemTime::now(),
            process_name: process_name.clone(),
            created_by_rustale: true,
        };
        tracker.insert(pid, process);
        println!("[Tracking] Registered process PID: {} ({})", pid, process_name);
    }
}

/// Remueve un proceso del tracker (cuando termina normalmente)
pub fn untrack_process(pid: u32) {
    if let Ok(mut tracker) = PID_TRACKER.lock() {
        if tracker.remove(&pid).is_some() {
            println!("[Tracking] Unregistered process PID: {}", pid);
        }
    }
}

/// Obtiene la lista de PIDs trackeados
pub fn get_tracked_pids() -> Vec<u32> {
    if let Ok(tracker) = PID_TRACKER.lock() {
        tracker.keys().copied().collect()
    } else {
        Vec::new()
    }
}

/// Limpia PIDs antiguos del tracker (más de 1 hora)
pub fn cleanup_old_pids() {
    if let Ok(mut tracker) = PID_TRACKER.lock() {
        let now = SystemTime::now();
        let one_hour = std::time::Duration::from_secs(3600);
        
        let before_count = tracker.len();
        tracker.retain(|_, process| {
            now.duration_since(process.start_time).unwrap_or_default() < one_hour
        });
        let after_count = tracker.len();
        
        if before_count != after_count {
            println!("[Tracking] Cleaned up {} old PIDs", before_count - after_count);
        }
    }
}

/// Valida si un PID pertenece realmente a un proceso de RusTale
pub fn is_tracked_process(pid: u32) -> bool {
    if let Ok(tracker) = PID_TRACKER.lock() {
        if tracker.get(&pid).is_some() {
            // Validación adicional: verificar que el proceso aún existe y es el correcto
            if cfg!(windows) {
                // En Windows podemos verificar el nombre del proceso
                if let Ok(output) = std::process::Command::new("tasklist")
                    .args(["/FI", format!("PID eq {}", pid).as_str(), "/FO", "CSV", "/NH"])
                    .output() 
                {
                    if output.status.success() {
                        let output_str = String::from_utf8_lossy(&output.stdout);
                        if output_str.contains("java.exe") || output_str.contains("java") {
                            return true;
                        }
                    }
                }
            } else {
                // En Unix verificamos con ps
                if let Ok(output) = std::process::Command::new("ps")
                    .args(["-p", &pid.to_string(), "-o", "comm="])
                    .output()
                {
                    if output.status.success() {
                        let output_str = String::from_utf8_lossy(&output.stdout);
                        if output_str.trim().contains("java") {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

/// Limpia todos los PIDs del tracker
pub fn clear_all_tracked_pids() {
    if let Ok(mut tracker) = PID_TRACKER.lock() {
        let count = tracker.len();
        tracker.clear();
        if count > 0 {
            println!("[Tracking] Cleared {} tracked PIDs", count);
        }
    }
}
