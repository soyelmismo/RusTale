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
            // En un contexto async como el de Tokio, necesitamos block_in_place
            // para operaciones bloqueantes de IO si estamos en el runtime de tokio.
            // Si no, std::fs::create_dir_all es suficiente.
            // NOTA: Movido a shared, quitamos la dependencia de tokio::task::block_in_place
            // si queremos que sea puramente agnostico, pero por ahora lo dejamos
            // o usamos std::fs directamente.
            let result = std::fs::create_dir_all(path);

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
            anyhow::bail!(
                "Directory verification failed: {} does not exist after creation attempt",
                path.display()
            );
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
        self.ensure_dir(&jre_path)
            .expect("Failed to create JRE directory")
    }

    /// Java executable path
    pub fn java_exec(&self) -> PathBuf {
        let bin_dir = self.jre().join("bin");
        self.ensure_dir(&bin_dir)
            .expect("Failed to create Java bin directory")
            .join(if cfg!(windows) { "java.exe" } else { "java" })
    }

    /// Butler executable path
    pub fn butler(&self) -> PathBuf {
        let butler_dir = self.tools().join("butler");
        self.ensure_dir(&butler_dir)
            .expect("Failed to create Butler directory")
            .join(if cfg!(windows) {
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
        self.ensure_dir(&path)
            .expect("Failed to create version directory");
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
        let _ = self
            .ensure_dir(&path)
            .expect("Failed to create mods directory");
        path
    }

    /// Disabled Mods for this version
    /// Path: RusTale/{channel}/{version}/DisabledMods
    /// Automatically creates directory if it doesn't exist
    pub fn disabled_mods_dir(&self, channel: &str, version_str: &str) -> PathBuf {
        let path = self.version_dir(channel, version_str).join("DisabledMods");
        let _ = self
            .ensure_dir(&path)
            .expect("Failed to create disabled mods directory");
        path
    }

    /// Core Patches directory
    /// Path: RusTale/{channel}/{version}/CorePatches/{ModID}/
    /// Automatically creates directory if it doesn't exist
    pub fn core_patches_dir(&self, channel: &str, version_str: &str) -> PathBuf {
        let path = self.version_dir(channel, version_str).join("CorePatches");
        let _ = self
            .ensure_dir(&path)
            .expect("Failed to create core patches directory");
        path
    }

    /// Returns the UserData directory
    /// Automatically creates directory if it doesn't exist
    pub fn user_data(&self) -> PathBuf {
        let path = self.root.join("UserData");
        let _ = self
            .ensure_dir(&path)
            .expect("Failed to create user data directory");
        path
    }

    /// Returns the channel directory
    /// Automatically creates directory if it doesn't exist
    pub fn channel_dir(&self, channel: &str) -> PathBuf {
        let path = self.root.join(channel);
        let _ = self
            .ensure_dir(&path)
            .expect("Failed to create channel directory");
        path
    }

    /// Java Agent path
    /// Ensures the tools directory exists but does not create the JAR file as a directory
    pub fn dualauth_agent(&self) -> PathBuf {
        let path = self.tools().join("dualauth-agent.jar");
        // Only ensure the parent directory (tools) exists, not the JAR file itself
        let _ = self
            .ensure_dir(&self.tools())
            .expect("Failed to create tools directory");
        path
    }

    
    /// Logs directory
    /// Automatically creates directory if it doesn't exist
    pub fn logs(&self) -> PathBuf {
        let path = self.root.join("logs");
        let _ = self
            .ensure_dir(&path)
            .expect("Failed to create logs directory");
        path
    }

    /// Staging directory for temporary files during patching
    /// Automatically creates directory if it doesn't exist
    pub fn staging(&self) -> PathBuf {
        let path = self.root.join("staging");
        let _ = self
            .ensure_dir(&path)
            .expect("Failed to create staging directory");
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    // === Cross-Platform Compatibility Tests ===
    // These tests document expected behavior on different platforms
    // and help catch regressions when running on different OSes

    #[test]
    fn test_path_separator_is_platform_aware() {
        // Document: Path separator should be handled by std::path automatically
        let dir = tempdir().expect("Failed to create temp dir");
        let paths = GamePaths::new(dir.path().to_path_buf());
        
        // On Windows: stable\latest, On Linux: stable/latest
        let version_dir = paths.version_dir("stable", "latest");
        
        // The important part: the path should exist and be accessible
        assert!(version_dir.exists(), "Version directory should exist regardless of platform");
        
        // Verify we can construct child paths correctly
        let child = version_dir.join("Client").join("test.txt");
        // Parent should exist (Client dir)
        if let Some(parent) = child.parent() {
            std::fs::create_dir_all(parent).ok();
            assert!(parent.exists() || std::fs::create_dir_all(parent).is_ok());
        }
    }

    #[test]
    fn test_executable_names_are_platform_specific() {
        let dir = tempdir().expect("Failed to create temp dir");
        let paths = GamePaths::new(dir.path().to_path_buf());
        
        let java = paths.java_exec();
        let butler = paths.butler();
        let client = paths.client_exe("stable", "latest");
        
        // On Windows: java.exe, butler.exe, HytaleClient.exe
        // On Linux/Mac: java, butler, HytaleClient
        if cfg!(windows) {
            assert!(java.to_string_lossy().ends_with("java.exe"), 
                "Windows should use .exe extension for java");
            assert!(butler.to_string_lossy().ends_with("butler.exe"),
                "Windows should use .exe extension for butler");
            assert!(client.to_string_lossy().ends_with("HytaleClient.exe"),
                "Windows should use .exe extension for client");
        } else {
            assert!(java.to_string_lossy().ends_with("java"),
                "Unix should not use extension for java");
            assert!(butler.to_string_lossy().ends_with("butler"),
                "Unix should not use extension for butler");
            assert!(client.to_string_lossy().ends_with("HytaleClient"),
                "Unix should not use extension for client");
        }
    }

    #[test]
    fn test_case_sensitivity_documentation() {
        // IMPORTANT: This test documents behavior but does NOT enforce case sensitivity
        // On Linux (case-sensitive FS): Mods/ and mods/ are different directories
        // On Windows (case-insensitive FS): Mods/ and mods/ are the same directory
        // 
        // CRITICAL: Always use the exact casing from GamePaths methods to avoid
        // issues when deploying to Linux servers!
        
        let dir = tempdir().expect("Failed to create temp dir");
        let paths = GamePaths::new(dir.path().to_path_buf());
        
        let mods_dir = paths.mods_dir("stable", "latest");
        let dir_name = mods_dir.file_name().unwrap().to_string_lossy().to_string();
        
        // The canonical name is "Mods" (capital M)
        assert_eq!(dir_name, "Mods", 
            "Mods directory should use canonical casing 'Mods' for Linux compatibility");
        
        // On Linux, creating "mods" (lowercase) would be a DIFFERENT directory
        // This test documents that GamePaths always uses the correct casing
    }

    #[test]
    fn test_path_with_unicode_characters() {
        // Test that paths with unicode characters work correctly
        // Important for international usernames or mod names
        let dir = tempdir().expect("Failed to create temp dir");
        let paths = GamePaths::new(dir.path().to_path_buf());
        
        // Test channel with unicode (e.g., "versión-特殊")
        let unicode_channel = "versión-特殊";
        let channel_dir = paths.channel_dir(unicode_channel);
        assert!(channel_dir.exists(), "Unicode channel names should work");
        
        // Verify the directory name is preserved
        assert!(channel_dir.to_string_lossy().contains(unicode_channel),
            "Unicode characters should be preserved in paths");
    }

    #[test]
    fn test_path_with_spaces() {
        // Test that paths with spaces work correctly
        // Common on Windows (e.g., "C:\\Program Files\\...")
        let dir = tempdir().expect("Failed to create temp dir");
        let paths = GamePaths::new(dir.path().to_path_buf());
        
        let channel_with_spaces = "pre release";
        let channel_dir = paths.channel_dir(channel_with_spaces);
        assert!(channel_dir.exists(), "Channel names with spaces should work");
    }

    #[test]
    fn test_path_normalization() {
        // Test that paths are normalized (no .. or . components in final path)
        let dir = tempdir().expect("Failed to create temp dir");
        let paths = GamePaths::new(dir.path().to_path_buf());
        
        let version_dir = paths.version_dir("stable", "latest");
        let path_str = version_dir.to_string_lossy();
        
        // Paths should not contain .. or ./ segments
        assert!(!path_str.contains(".."), "Paths should not contain '..' segments");
        assert!(!path_str.contains("./"), "Paths should not contain './' segments");
    }

    #[test]
    fn test_absolute_vs_relative_paths() {
        // Document that GamePaths works with both absolute and relative paths
        // but recommends absolute paths for reliability
        
        // With absolute path
        let abs_dir = tempdir().expect("Failed to create temp dir");
        let abs_paths = GamePaths::new(abs_dir.path().to_path_buf());
        assert!(abs_paths.root.is_absolute(), 
            "Temp dir should give absolute path");
        
        // With relative path (should work but not recommended)
        let rel_paths = GamePaths::new(PathBuf::from("."));
        assert!(rel_paths.root.is_relative(),
            "Explicit relative path should remain relative");
        
        // Both should create directories successfully
        let abs_version = abs_paths.version_dir("stable", "latest");
        let _rel_version = rel_paths.version_dir("stable", "latest");
        
        // Cleanup relative path if created
        let _ = std::fs::remove_dir_all("./stable");
        
        assert!(abs_version.exists(), "Absolute path should create version dir");
    }

    // === Original Unit Tests ===

    #[test]
    fn test_game_paths_new() {
        let path = PathBuf::from("/test/path");
        let paths = GamePaths::new(path.clone());
        assert_eq!(paths.root, path);
    }

    #[test]
    fn test_tools_path() {
        let dir = tempdir().expect("Failed to create temp dir");
        let paths = GamePaths::new(dir.path().to_path_buf());
        let tools = paths.tools();
        assert!(tools.ends_with("tools"));
    }

    #[test]
    fn test_version_dir_latest() {
        let dir = tempdir().expect("Failed to create temp dir");
        let paths = GamePaths::new(dir.path().to_path_buf());
        let version_dir = paths.version_dir("stable", "latest");
        assert!(version_dir.ends_with("stable/latest"));
        assert!(version_dir.exists());
    }

    #[test]
    fn test_version_dir_numeric() {
        let dir = tempdir().expect("Failed to create temp dir");
        let paths = GamePaths::new(dir.path().to_path_buf());
        let version_dir = paths.version_dir("stable", "42");
        assert!(version_dir.ends_with("stable/42"));
        assert!(version_dir.exists());
    }

    #[test]
    fn test_version_dir_zero_converts_to_latest() {
        let dir = tempdir().expect("Failed to create temp dir");
        let paths = GamePaths::new(dir.path().to_path_buf());
        let version_dir = paths.version_dir("stable", "0");
        assert!(version_dir.ends_with("stable/latest"));
    }

    #[test]
    fn test_channel_dir() {
        let dir = tempdir().expect("Failed to create temp dir");
        let paths = GamePaths::new(dir.path().to_path_buf());
        let channel = paths.channel_dir("pre-release");
        assert!(channel.ends_with("pre-release"));
        assert!(channel.exists());
    }

    #[test]
    fn test_mods_dir() {
        let dir = tempdir().expect("Failed to create temp dir");
        let paths = GamePaths::new(dir.path().to_path_buf());
        let mods = paths.mods_dir("stable", "latest");
        assert!(mods.ends_with("Mods"));
        assert!(mods.exists());
    }

    #[test]
    fn test_disabled_mods_dir() {
        let dir = tempdir().expect("Failed to create temp dir");
        let paths = GamePaths::new(dir.path().to_path_buf());
        let disabled = paths.disabled_mods_dir("stable", "latest");
        assert!(disabled.ends_with("DisabledMods"));
        assert!(disabled.exists());
    }

    #[test]
    fn test_core_patches_dir() {
        let dir = tempdir().expect("Failed to create temp dir");
        let paths = GamePaths::new(dir.path().to_path_buf());
        let patches = paths.core_patches_dir("stable", "latest");
        assert!(patches.ends_with("CorePatches"));
        assert!(patches.exists());
    }

    #[test]
    fn test_version_json_path() {
        let dir = tempdir().expect("Failed to create temp dir");
        let paths = GamePaths::new(dir.path().to_path_buf());
        let json = paths.version_json("stable");
        assert!(json.ends_with("stable/version.json"));
    }

    #[test]
    fn test_user_data_dir() {
        let dir = tempdir().expect("Failed to create temp dir");
        let paths = GamePaths::new(dir.path().to_path_buf());
        let user = paths.user_data();
        assert!(user.ends_with("UserData"));
        assert!(user.exists());
    }

    #[test]
    fn test_logs_dir() {
        let dir = tempdir().expect("Failed to create temp dir");
        let paths = GamePaths::new(dir.path().to_path_buf());
        let logs = paths.logs();
        assert!(logs.ends_with("logs"));
        assert!(logs.exists());
    }

    #[test]
    fn test_staging_dir() {
        let dir = tempdir().expect("Failed to create temp dir");
        let paths = GamePaths::new(dir.path().to_path_buf());
        let staging = paths.staging();
        assert!(staging.ends_with("staging"));
        assert!(staging.exists());
    }

    #[test]
    fn test_dualauth_agent_path() {
        let dir = tempdir().expect("Failed to create temp dir");
        let paths = GamePaths::new(dir.path().to_path_buf());
        let agent = paths.dualauth_agent();
        assert!(agent.ends_with("dualauth-agent.jar"));
        // Note: The JAR file itself should NOT exist as a directory
    }

    #[test]
    fn test_ensure_dir_creates_directory() {
        let dir = tempdir().expect("Failed to create temp dir");
        let paths = GamePaths::new(dir.path().to_path_buf());
        let new_dir = dir.path().join("new_directory");
        let result = paths.ensure_dir(&new_dir);
        assert!(result.is_ok());
        assert!(new_dir.exists());
        assert!(new_dir.is_dir());
    }

    #[test]
    fn test_ensure_dir_existing_directory() {
        let dir = tempdir().expect("Failed to create temp dir");
        let paths = GamePaths::new(dir.path().to_path_buf());
        // Create a directory first
        let existing = dir.path().join("existing");
        std::fs::create_dir_all(&existing).expect("Failed to create dir");
        
        // ensure_dir should succeed on existing directory
        let result = paths.ensure_dir(&existing);
        assert!(result.is_ok());
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // PROPERTY-BASED TESTS (using proptest)
    // These tests generate hundreds of random inputs to find edge cases.
    // CRITICAL for cross-platform paths - the #1 source of bugs in launchers.
    // ═══════════════════════════════════════════════════════════════════════════════

    #[cfg(test)]
    mod property_tests {
        use super::*;
        use proptest::prelude::*;

        /// Generate valid path segments (no invalid chars)
        fn valid_path_segment() -> impl Strategy<Value = String> {
            "[a-zA-Z0-9_-]{1,20}"
        }

        /// Generate path segments with spaces (common on Windows)
        fn path_with_spaces() -> impl Strategy<Value = String> {
            "[a-zA-Z0-9_ -]{1,30}"
        }

        /// Generate channel names
        fn channel_name() -> impl Strategy<Value = String> {
            prop_oneof![
                Just("stable".to_string()),
                Just("pre-release".to_string()),
                Just("release".to_string()),
                path_with_spaces(),
            ]
        }

        /// Generate version strings
        fn version_string() -> impl Strategy<Value = String> {
            prop_oneof![
                Just("latest".to_string()),
                Just("0".to_string()),
                "[0-9]{1,4}",
            ]
        }

        proptest! {
            /// Property: Any valid channel/version creates a valid path
            #[test]
            fn proptest_version_dir_creates_valid_path(
                channel in channel_name(),
                version in version_string()
            ) {
                let dir = tempdir().expect("Failed to create temp dir");
                let paths = GamePaths::new(dir.path().to_path_buf());
                
                let result = std::panic::catch_unwind(|| {
                    paths.version_dir(&channel, &version)
                });
                
                prop_assert!(result.is_ok(), "version_dir should not panic");
                
                let version_dir = result.unwrap();
                prop_assert!(version_dir.exists(), "version_dir should exist");
                prop_assert!(version_dir.is_dir(), "version_dir should be a directory");
            }

            /// Property: Paths with spaces work (Windows common case)
            #[test]
            fn proptest_paths_with_spaces_work(segment in path_with_spaces()) {
                if segment.trim().is_empty() {
                    return Ok(());
                }
                
                let dir = tempdir().expect("Failed to create temp dir");
                let paths = GamePaths::new(dir.path().to_path_buf());
                
                let result = std::panic::catch_unwind(|| {
                    paths.channel_dir(&segment)
                });
                
                prop_assert!(result.is_ok(), "channel_dir should handle spaces");
                
                if let Ok(channel_dir) = result {
                    prop_assert!(channel_dir.exists(), "Channel dir with spaces should exist");
                }
            }

            /// Property: version "0" normalizes to "latest" consistently
            #[test]
            fn proptest_version_zero_normalizes_to_latest(channel in valid_path_segment()) {
                let dir = tempdir().expect("Failed to create temp dir");
                let paths = GamePaths::new(dir.path().to_path_buf());
                
                let version_zero = paths.version_dir(&channel, "0");
                let version_latest = paths.version_dir(&channel, "latest");
                
                prop_assert_eq!(version_zero, version_latest, 
                    "Version 0 and latest should resolve to same path");
            }

            /// Property: mods_dir always uses canonical casing "Mods"
            #[test]
            fn proptest_mods_dir_uses_canonical_casing(
                channel in valid_path_segment(),
                version in version_string()
            ) {
                let dir = tempdir().expect("Failed to create temp dir");
                let paths = GamePaths::new(dir.path().to_path_buf());
                
                let mods_dir = paths.mods_dir(&channel, &version);
                let dir_name = mods_dir.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                
                prop_assert_eq!(dir_name, "Mods", 
                    "Mods directory MUST use 'Mods' for Linux compatibility");
            }

            /// Property: Path creation is idempotent
            #[test]
            fn proptest_path_creation_is_idempotent(
                channel in valid_path_segment(),
                version in version_string()
            ) {
                let dir = tempdir().expect("Failed to create temp dir");
                let paths = GamePaths::new(dir.path().to_path_buf());
                
                let path1 = paths.version_dir(&channel, &version);
                let path2 = paths.version_dir(&channel, &version);
                
                prop_assert_eq!(path1, path2.clone(), "Repeated calls should return identical paths");
                prop_assert!(path2.exists(), "Path should exist after calls");
            }

            /// Property: Absolute root produces absolute paths
            #[test]
            fn proptest_absolute_root_produces_absolute_paths(
                channel in valid_path_segment(),
                version in version_string()
            ) {
                let dir = tempdir().expect("Failed to create temp dir");
                let paths = GamePaths::new(dir.path().to_path_buf());
                
                let version_dir = paths.version_dir(&channel, &version);
                let mods_dir = paths.mods_dir(&channel, &version);
                let tools_dir = paths.tools();
                
                prop_assert!(version_dir.is_absolute(), "version_dir should be absolute");
                prop_assert!(mods_dir.is_absolute(), "mods_dir should be absolute");
                prop_assert!(tools_dir.is_absolute(), "tools_dir should be absolute");
            }
        }

        #[cfg(windows)]
        proptest! {
            /// Property: Windows paths don't contain invalid characters
            #[test]
            fn proptest_windows_paths_no_invalid_chars(
                channel in valid_path_segment(),
                version in version_string()
            ) {
                let dir = tempdir().expect("Failed to create temp dir");
                let paths = GamePaths::new(dir.path().to_path_buf());
                
                let version_dir = paths.version_dir(&channel, &version);
                let path_str = version_dir.to_string_lossy();
                
                let has_invalid = path_str.chars().any(|c| {
                    matches!(c, '<' | '>' | '"' | '|' | '?' | '*')
                });
                
                prop_assert!(!has_invalid, "Path should not contain Windows-invalid chars");
            }
        }

        #[cfg(unix)]
        proptest! {
            /// Property: Unix paths don't contain null bytes
            #[test]
            fn proptest_unix_paths_no_null_bytes(
                channel in valid_path_segment(),
                version in version_string()
            ) {
                let dir = tempdir().expect("Failed to create temp dir");
                let paths = GamePaths::new(dir.path().to_path_buf());
                
                let version_dir = paths.version_dir(&channel, &version);
                let path_str = version_dir.to_string_lossy();
                
                prop_assert!(!path_str.contains('\0'), "Path should not contain null bytes");
            }
        }
    }
}
