//! Patch providers implementations
//! 
//! This module implements a plug-and-play provider system where providers are
//! self-contained units that can be added/removed without modifying other files.
//! 
//! To add a new provider:
//! 1. Create a new file (e.g., `p_new.rs`)
//! 2. Implement the `PatchProvider` trait
//! 3. Add the module and call `register_provider!()` here
//! 4. That's it! No other files need to be modified.

use std::sync::Arc;
use crate::patch_api::traits::PatchProvider;

// Public provider - always available
pub mod hytale;

// Security-enabled providers - obfuscated names
#[cfg(feature = "security")]
pub mod p0;
#[cfg(feature = "security")]
pub mod p1;
#[cfg(feature = "security")]
pub mod p2;
#[cfg(feature = "security")]
pub mod p3;

// Re-export all providers for backwards compatibility
pub use hytale::HytaleProvider;

#[cfg(feature = "security")]
pub use p0::Provider0;
#[cfg(feature = "security")]
pub use p1::Provider1;
#[cfg(feature = "security")]
pub use p2::Provider2;
#[cfg(feature = "security")]
pub use p3::Provider3;

// Note: Provider priorities are now defined in each provider's implementation
// via the PatchProvider::priority() method. No separate constant table needed.

// ============================================================================
// PLUG-AND-PLAY PROVIDER REGISTRY
// ============================================================================

/// Returns all available providers as trait objects, ordered by priority (highest first).
/// 
/// This is the single source of truth for provider instantiation.
/// Adding a new provider only requires modifying this function and the module declarations above.
/// 
/// # Returns
/// A vector of `Arc<dyn PatchProvider>` ready to use, sorted by priority.
pub fn get_all_providers() -> Vec<Arc<dyn PatchProvider>> {
    let mut providers: Vec<Arc<dyn PatchProvider>> = Vec::new();

    #[cfg(feature = "security")]
    {
        providers.push(Arc::new(Provider0::new()));
        //providers.push(Arc::new(Provider1::new()));
        //providers.push(Arc::new(Provider2::new()));
        //providers.push(Arc::new(Provider3::new()));
    }

    // Sort by priority (highest first)
    providers.sort_by(|a, b| b.priority().cmp(&a.priority()));
    
    providers
}

/// Returns the number of registered providers
pub fn provider_count() -> usize {
    #[cfg(feature = "security")]
    {
        4
    }
    #[cfg(not(feature = "security"))]
    {
        0
    }
}
