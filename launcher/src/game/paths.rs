use std::path::PathBuf;

/// Centralized path management for game files
/// This is the single source of truth for all file locations
#[derive(Clone, Debug)]
pub struct GamePaths {
    pub root: PathBuf,
}

impl GamePaths {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Ensures a directory exists, creating it if necessary
    /// This is the centralized way to handle directory creation
    /// Blocks current thread until directory creation is complete
    /// Returns the path for chaining operations
    /// Returns an error if directory creation fails
    pub fn ensure_dir(&self, path: &PathBuf) -> anyhow::Result<PathBuf> {
        if !path.exists() {
            println!("[Paths] Creating directory: {}", path.display());
            let result = tokio::task::block_in_place(|| {
                std::fs::create_dir_all(path)
            });
            
            match result {
                Ok(_) => {
                    println!("[Paths] Directory created: {}", path.display());
                }
                Err(e) => {
                    anyhow::bail!("Failed to create directory {}: {}", path.display(), e);
                }
            }
        }
        
        // Double-verify the directory exists to prevent race conditions
        if !path.exists() {
            anyhow::bail!("Directory verification failed: {} does not exist after creation attempt", path.display());
        }
        
        if !path.is_dir() {
            anyhow::bail!("Path exists but is not a directory: {}", path.display());
        }
        
        Ok(path.to_path_buf())
    }

    /// Tools directory (contains JRE, Butler, etc.)
    pub fn tools(&self) -> PathBuf {
        self.root.join("tools")
    }

    /// JRE installation directory
    pub fn jre(&self) -> PathBuf {
        let jre_path = self.tools().join("jre").join("latest");
        self.ensure_dir(&jre_path).expect("Failed to create JRE directory")
    }

    /// Java executable path
    pub fn java_exec(&self) -> PathBuf {
        let bin_dir = self.jre().join("bin");
        self.ensure_dir(&bin_dir).expect("Failed to create Java bin directory").join(if cfg!(windows) {
            "java.exe"
        } else {
            "java"
        })
    }

    /// Butler executable path
    pub fn butler(&self) -> PathBuf {
        let butler_dir = self.tools().join("butler");
        self.ensure_dir(&butler_dir).expect("Failed to create Butler directory").join(if cfg!(windows) {
            "butler.exe"
        } else {
            "butler"
        })
    }

    /// Returns the directory where a version SHOULD be installed
    /// - If version_str is "latest" or "0", returns .../channel/latest
    /// - Otherwise returns .../channel/{version_str}
    /// Automatically creates directory if it doesn't exist
    pub fn version_dir(&self, channel: &str, version_str: &str) -> PathBuf {
        let folder_name = if version_str == "0" || version_str == "latest" {
            "latest"
        } else {
            version_str
        };
        let path = self.channel_dir(channel).join(folder_name);
        self.ensure_dir(&path).expect("Failed to create version directory");
        path
    }

    /// Returns the path to version.json for a channel
    /// This file is stored at the channel root level
    pub fn version_json(&self, channel: &str) -> PathBuf {
        self.channel_dir(channel).join("version.json")
    }

    /// Returns the path to the game client executable
    /// Automatically creates necessary directories if they don't exist
    pub fn client_exe(&self, channel: &str, version_str: &str) -> PathBuf {
        let name = if cfg!(windows) {
            "HytaleClient.exe"
        } else {
            "HytaleClient"
        };
        let version_dir = self.version_dir(channel, version_str);
        version_dir.join("Client").join(name)
    }

    // --- ISOLATED MOD MANAGEMENT ---

    /// Mods (.jar/.zip) for this version
    /// Path: RusTale/{channel}/{version}/Mods
    /// Automatically creates directory if it doesn't exist
    pub fn mods_dir(&self, channel: &str, version_str: &str) -> PathBuf {
        let path = self.version_dir(channel, version_str).join("Mods");
        let _ = self.ensure_dir(&path).expect("Failed to create mods directory");
        path
    }

    /// Disabled Mods for this version
    /// Path: RusTale/{channel}/{version}/DisabledMods
    /// Automatically creates directory if it doesn't exist
    pub fn disabled_mods_dir(&self, channel: &str, version_str: &str) -> PathBuf {
        let path = self.version_dir(channel, version_str).join("DisabledMods");
        let _ = self.ensure_dir(&path).expect("Failed to create disabled mods directory");
        path
    }

    /// Core Patches directory
    /// Path: RusTale/{channel}/{version}/CorePatches/{ModID}/
    /// Automatically creates directory if it doesn't exist
    pub fn core_patches_dir(&self, channel: &str, version_str: &str) -> PathBuf {
        let path = self.version_dir(channel, version_str).join("CorePatches");
        let _ = self.ensure_dir(&path).expect("Failed to create core patches directory");
        path
    }

    /// Returns the UserData directory
    /// Automatically creates directory if it doesn't exist
    pub fn user_data(&self) -> PathBuf {
        let path = self.root.join("UserData");
        let _ = self.ensure_dir(&path).expect("Failed to create user data directory");
        path
    }

    /// Returns the channel directory
    /// Automatically creates directory if it doesn't exist
    pub fn channel_dir(&self, channel: &str) -> PathBuf {
        let path = self.root.join(channel);
        let _ = self.ensure_dir(&path).expect("Failed to create channel directory");
        path
    }

    /// Java Agent path
    /// Ensures the tools directory exists but does not create the JAR file as a directory
    pub fn dualauth_agent(&self) -> PathBuf {
        let path = self.tools().join("dualauth-agent.jar");
        // Only ensure the parent directory (tools) exists, not the JAR file itself
        let _ = self.ensure_dir(&self.tools()).expect("Failed to create tools directory");
        path
    }

    /// Game directory (root for all game installations)
    pub fn game_dir(&self) -> PathBuf {
        self.root.clone()
    }

    /// Logs directory
    /// Automatically creates directory if it doesn't exist
    pub fn logs(&self) -> PathBuf {
        let path = self.root.join("logs");
        let _ = self.ensure_dir(&path).expect("Failed to create logs directory");
        path
    }

    /// Staging directory for temporary files during patching
    /// Automatically creates directory if it doesn't exist
    pub fn staging(&self) -> PathBuf {
        let path = self.root.join("staging");
        let _ = self.ensure_dir(&path).expect("Failed to create staging directory");
        path
    }
}
