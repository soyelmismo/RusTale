use anyhow::Result;
use sysinfo::{System, Disks};

pub struct PreLaunchChecks;

impl PreLaunchChecks {
    /// Performs system validation before launching
    /// Returns Ok if system is healthy, Err with user-friendly message otherwise
    pub fn validate_system_requirements(
        min_memory_gb: u32,
        install_dir: &std::path::Path
    ) -> Result<()> {
        let mut sys = System::new();
        sys.refresh_memory();

        // 1. RAM Check
        let available_ram = sys.available_memory(); // Bytes
        let min_required = (min_memory_gb as u64) * 1024 * 1024 * 1024;
        
        // Allow a small buffer (e.g., if user sets 4GB and has 3.9GB available, warn but maybe allow)
        // Ideally, we check strict requirement + OS overhead
        if available_ram < min_required {
            // Log warning but don't hard block unless it's critical
            println!("[Checks] WARNING: Low memory. Available: {} MB, Requested: {} MB", 
                available_ram / 1024 / 1024, min_required / 1024 / 1024);
        }

        // 2. Disk Space Check
        let disks = Disks::new_with_refreshed_list();
        // Find the disk containing the install_dir
        if let Some(disk) = disks.iter().find(|d| install_dir.starts_with(d.mount_point())) {
            let available_space = disk.available_space();
            let min_space = 500 * 1024 * 1024; // 500 MB for runtime files/logs
            
            if available_space < min_space {
                anyhow::bail!(
                    "Insufficient disk space. Available: {} MB, Required: 500 MB", 
                    available_space / 1024 / 1024
                );
            }
        }

        Ok(())
    }
}
