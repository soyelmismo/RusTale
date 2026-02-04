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

    /// Tools directory (contains JRE, Butler, etc.)
    pub fn tools(&self) -> PathBuf {
        self.root.join("tools")
    }

    /// JRE installation directory
    pub fn jre(&self) -> PathBuf {
        self.tools().join("jre").join("latest")
    }

    /// Java executable path
    pub fn java_exec(&self) -> PathBuf {
        let bin_dir = self.jre().join("bin");
        if cfg!(windows) {
            bin_dir.join("java.exe")
        } else {
            bin_dir.join("java")
        }
    }

    /// Butler executable path
    pub fn butler(&self) -> PathBuf {
        let name = if cfg!(windows) {
            "butler.exe"
        } else {
            "butler"
        };
        self.tools().join("butler").join(name)
    }

    /// Returns the directory where a version SHOULD be installed
    /// - If version_str is "latest" or "0", returns .../channel/latest
    /// - Otherwise returns .../channel/{version_str}
    pub fn version_dir(&self, channel: &str, version_str: &str) -> PathBuf {
        let folder_name = if version_str == "0" || version_str == "latest" {
            "latest"
        } else {
            version_str
        };
        self.channel_dir(channel).join(folder_name)
    }

    /// Returns the path to version.json for a channel
    /// This file is stored at the channel root level
    pub fn version_json(&self, channel: &str) -> PathBuf {
        self.channel_dir(channel).join("version.json")
    }

    /// Returns the path to the game client executable
    pub fn client_exe(&self, channel: &str, version_str: &str) -> PathBuf {
        let name = if cfg!(windows) {
            "HytaleClient.exe"
        } else {
            "HytaleClient"
        };
        self.version_dir(channel, version_str)
            .join("Client")
            .join(name)
    }

    // --- ISOLATED MOD MANAGEMENT ---

    /// Mods (.jar/.zip) for this version
    /// Path: RusTale/{channel}/{version}/Mods
    pub fn mods_dir(&self, channel: &str, version_str: &str) -> PathBuf {
        self.version_dir(channel, version_str).join("Mods")
    }

    /// Disabled Mods for this version
    /// Path: RusTale/{channel}/{version}/DisabledMods
    pub fn disabled_mods_dir(&self, channel: &str, version_str: &str) -> PathBuf {
        self.version_dir(channel, version_str).join("DisabledMods")
    }

    /// Core Patches directory
    /// Path: RusTale/{channel}/{version}/CorePatches/{ModID}/
    pub fn core_patches_dir(&self, channel: &str, version_str: &str) -> PathBuf {
        self.version_dir(channel, version_str).join("CorePatches")
    }

    /// Returns the UserData directory
    pub fn user_data(&self) -> PathBuf {
        self.root.join("UserData")
    }

    /// Returns the channel directory
    pub fn channel_dir(&self, channel: &str) -> PathBuf {
        self.root.join(channel)
    }

    /// Java Agent path
    pub fn dualauth_agent(&self) -> PathBuf {
        self.tools().join("dualauth-agent.jar")
    }
}
