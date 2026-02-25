use rustale_shared::config::GameSettings;
use rustale_shared::profiles::ProfilesConfig;
use serde::{Serialize, de::DeserializeOwned};
use std::path::PathBuf;
pub mod lifecycle;

// Re-export specific items for convenience
pub use rustale_shared::config::{
    BaseThemeMode, LauncherConfig, OnlineFixMode, ThemeConfig, get_app_dir, get_bootstrap_path,
    get_identity_dir, get_server_root_dir, save_bootstrap_path,
};

// ============================================================================
// ATOMIC FILE I/O — PREVENTS 0-BYTE CATASTROPHE
//
// Every write goes through a 3-phase commit:
//   1. Serialize + validate (abort early if content is bad)
//   2. Write to a .tmp file + fsync  (never touches the real file)
//   3. Rename .bak ← original, then .tmp → original (atomic on POSIX)
//
// On load, if the primary file is missing/empty/corrupt, the .bak is tried
// automatically so users never lose more than one save cycle of data.
// ============================================================================

/// Minimum plausible size (bytes) for a valid settings/profile TOML.
/// A completely default GameSettings serialises to ~500+ bytes.
/// Anything below this threshold is treated as corrupt.
const MIN_VALID_TOML_SIZE: usize = 32;

// ---------------------------------------------------------------------------
//  LOAD
// ---------------------------------------------------------------------------

pub async fn load_profiles() -> ProfilesConfig {
    load_toml_with_recovery::<ProfilesConfig>("profiles.toml")
}

pub async fn load_settings() -> GameSettings {
    load_settings_sync()
}

pub fn load_settings_sync() -> GameSettings {
    let config = load_toml_with_recovery::<GameSettings>("settings.toml");

    let mut safe_config = config;
    if safe_config.width < 100 {
        safe_config.width = 480;
    }
    if safe_config.height < 100 {
        safe_config.height = 390;
    }

    safe_config
}

/// Generic TOML loader with automatic backup recovery.
///
/// Priority order:
///   1. Primary file  (e.g. `settings.toml`)
///   2. Backup file   (e.g. `settings.toml.bak`)
///   3. `T::default()`
fn load_toml_with_recovery<T: DeserializeOwned + Default>(filename: &str) -> T {
    let primary = get_path(filename);
    let backup = get_path(&format!("{}.bak", filename));

    // --- Try primary ---
    if let Some(cfg) = try_load_toml::<T>(&primary) {
        return cfg;
    }

    // --- Primary failed — try backup ---
    eprintln!(
        "[System] WARNING: '{}' is missing or corrupt, attempting recovery from backup...",
        filename
    );

    if let Some(cfg) = try_load_toml::<T>(&backup) {
        // Restore the backup as the primary so the next save cycle has a
        // healthy base to work from.
        if let Err(e) = std::fs::copy(&backup, &primary) {
            eprintln!(
                "[System] WARNING: could not restore backup → primary: {}",
                e
            );
        } else {
            eprintln!(
                "[System] Recovered '{}' from backup successfully.",
                filename
            );
        }
        return cfg;
    }

    eprintln!(
        "[System] WARNING: both '{}' and its backup are unusable — using defaults.",
        filename
    );
    T::default()
}

/// Attempt to read and parse a single TOML file.  Returns `None` on any
/// failure (missing, empty, corrupt, too small, parse error).
fn try_load_toml<T: DeserializeOwned>(path: &PathBuf) -> Option<T> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return None,
    };

    // Reject suspiciously small files (likely 0-byte truncation artifacts)
    if content.trim().len() < MIN_VALID_TOML_SIZE {
        eprintln!(
            "[System] Rejecting '{}': only {} bytes (minimum {})",
            path.display(),
            content.len(),
            MIN_VALID_TOML_SIZE
        );
        return None;
    }

    match toml::from_str::<T>(&content) {
        Ok(cfg) => Some(cfg),
        Err(e) => {
            eprintln!("[System] Failed to parse '{}': {}", path.display(), e);
            None
        }
    }
}

// ---------------------------------------------------------------------------
//  SAVE
// ---------------------------------------------------------------------------

pub async fn save_profiles(cfg: &ProfilesConfig) -> anyhow::Result<()> {
    atomic_save_toml(cfg, "profiles.toml")
}

pub async fn save_settings(cfg: &GameSettings) -> anyhow::Result<()> {
    save_settings_sync(cfg)
}

pub fn save_settings_sync(cfg: &GameSettings) -> anyhow::Result<()> {
    atomic_save_toml(cfg, "settings.toml")
}

/// Atomic TOML save with backup.
///
/// Steps:
///   1. Serialize to TOML string.
///   2. Validate: content must be non-empty and re-parseable.
///   3. Write to `<file>.tmp` and `fsync`.
///   4. If `<file>` already exists and is healthy, rename it to `<file>.bak`.
///   5. Rename `<file>.tmp` → `<file>` (atomic on same filesystem).
///
/// If any step fails, the original file is untouched.
fn atomic_save_toml<T: Serialize + DeserializeOwned>(
    data: &T,
    filename: &str,
) -> anyhow::Result<()> {
    use std::io::Write;

    let toml_str = toml::to_string_pretty(data)?;

    // ── Phase 1: Content validation ──────────────────────────────────────
    if toml_str.trim().len() < MIN_VALID_TOML_SIZE {
        anyhow::bail!(
            "Refusing to save '{}': serialised content is suspiciously small ({} bytes). \
             This is likely a bug — aborting to protect existing data.",
            filename,
            toml_str.len()
        );
    }

    // Round-trip check: make sure what we serialised can be deserialised back.
    if toml::from_str::<T>(&toml_str).is_err() {
        anyhow::bail!(
            "Refusing to save '{}': serialised TOML fails round-trip parse. \
             Aborting to protect existing data.",
            filename
        );
    }

    // ── Phase 2: Paths ───────────────────────────────────────────────────
    let target = get_path(filename);
    let tmp = get_path(&format!("{}.tmp", filename));
    let backup = get_path(&format!("{}.bak", filename));

    if let Some(parent) = target.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // ── Phase 3: Write to temp file + fsync ──────────────────────────────
    {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(toml_str.as_bytes())?;
        file.flush()?;
        file.sync_all()?; // fsync — data is on disk before we proceed
    }

    // ── Phase 4: Verify the temp file is readable and non-empty ──────────
    // (Guards against filesystem-level corruption or disk-full scenarios
    //  where the file was created but the data never landed.)
    {
        let written = std::fs::read_to_string(&tmp)?;
        if written.trim().len() < MIN_VALID_TOML_SIZE {
            // Clean up the bad temp file and bail
            let _ = std::fs::remove_file(&tmp);
            anyhow::bail!(
                "Post-write verification failed for '{}': temp file has {} bytes on disk. \
                 Disk full?  Original file is untouched.",
                filename,
                written.len()
            );
        }
    }

    // ── Phase 5: Rotate backup ───────────────────────────────────────────
    // Only promote the current file to .bak if it's actually healthy.
    // If it's already 0-byte / corrupt, don't overwrite a potentially
    // good .bak with garbage.
    if target.exists() {
        let is_healthy = std::fs::metadata(&target)
            .map(|m| m.len() as usize >= MIN_VALID_TOML_SIZE)
            .unwrap_or(false);

        if is_healthy {
            // On Windows, rename fails if dest exists, so remove first.
            let _ = std::fs::remove_file(&backup);
            if let Err(e) = std::fs::rename(&target, &backup) {
                // Non-fatal: we can still proceed without a backup.
                eprintln!(
                    "[System] WARNING: could not create backup of '{}': {}",
                    filename, e
                );
            }
        }
    }

    // ── Phase 6: Atomic rename tmp → target ──────────────────────────────
    // On POSIX this is atomic (single rename syscall).
    // On Windows `rename` is not guaranteed atomic but is still far safer
    // than truncate-then-write because the destination only disappears for
    // a tiny window, and we already have the .bak as a safety net.
    //
    // On Windows, remove the target first if it still exists (rename won't
    // overwrite an existing file on Windows).
    #[cfg(windows)]
    {
        let _ = std::fs::remove_file(&target);
    }

    std::fs::rename(&tmp, &target).map_err(|e| {
        // If rename fails, the .tmp file is still intact and the original
        // (or .bak) is still intact. Nothing is lost.
        anyhow::anyhow!(
            "CRITICAL: atomic rename of '{}' failed: {}. \
             Data is safe in '{}.tmp'.  Original is untouched.",
            filename,
            e,
            filename
        )
    })?;

    Ok(())
}

// ---------------------------------------------------------------------------
//  INITIALISATION HELPERS
// ---------------------------------------------------------------------------

pub struct InitializationConfig {
    pub quickplay: bool,
}

pub fn load_initialization_config_sync() -> InitializationConfig {
    let settings = load_settings_sync();
    InitializationConfig {
        quickplay: settings.quickplay,
    }
}

pub fn load_width_height() -> (f32, f32) {
    let settings = load_settings_sync();
    (settings.width as f32, settings.height as f32)
}

// ---------------------------------------------------------------------------
//  INTERNAL
// ---------------------------------------------------------------------------

fn get_path(filename: &str) -> PathBuf {
    get_app_dir().join(filename)
}

// Function helpers
pub fn default_lang() -> String {
    "en-US".to_string()
}
pub fn default_scale() -> f32 {
    1.0
}
pub fn default_true() -> bool {
    true
}
