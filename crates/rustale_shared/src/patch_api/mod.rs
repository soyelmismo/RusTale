//! Patch API Module - Download and manage game patches
//! 
//! This module provides a unified interface for downloading game patches
//! from multiple mirrors with automatic fallback and version discovery.

// Provider implementations
pub mod providers;

// Core modules
pub mod install;
pub mod integrity_checker;
pub mod patch_downloader;
pub mod shared_cache;
pub mod traits;
pub mod utils;
pub mod mod_manager;
pub mod types;
pub mod version_manager;

// Frontend (high-level API)
pub mod frontend;

// Re-export providers from the providers subdirectory
pub use providers::{
    HytaleProvider,
    PROVIDER_PRIORITIES,
    get_provider_priority,
};

#[cfg(feature = "security")]
pub use providers::{
    Provider0,
    Provider1,
    Provider2,
    Provider3,
};

// Re-export from mod_manager (now includes MirrorManager and utilities)
pub use mod_manager::{
    PatchApiManager,
    MirrorManager,
    MirrorConfig,
    MirrorStats,
    ProviderVersionInfo,
    normalize_architecture,
    normalize_os,
    extract_version_number,
    build_manifest_path,
    build_patch_path,
};

// Re-export install module
pub use install::{
    InstallPolicy,
    is_game_installed,
    get_installed_versions,
};

// Re-export frontend
pub use frontend::PatchApiFrontend;

// Re-export other types
pub use crate::butler::ButlerInstaller;
pub use crate::java::JreInstaller;
pub use integrity_checker::IntegrityChecker;
pub use patch_downloader::PatchDownloader;
pub use shared_cache::{get_shared_cache, init_shared_cache, CacheStats, SharedCacheManager};
pub use traits::PatchProvider;
pub use utils::*;
pub use version_manager::VersionManager;
pub use types::*;