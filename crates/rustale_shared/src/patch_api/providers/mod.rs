//! Patch providers implementations

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

// Re-export all providers
pub use hytale::HytaleProvider;

#[cfg(feature = "security")]
pub use p0::Provider0;
#[cfg(feature = "security")]
pub use p1::Provider1;
#[cfg(feature = "security")]
pub use p2::Provider2;
#[cfg(feature = "security")]
pub use p3::Provider3;

/// Provider priority constants
pub const PROVIDER_PRIORITIES: &[(&str, i32)] = &[
    #[cfg(feature = "security")]
    ("E", 100),
    #[cfg(feature = "security")]
    ("S", 90),
    #[cfg(feature = "security")]
    ("H1", 80),
    #[cfg(feature = "security")]
    ("H2", 75),
    #[cfg(feature = "security")]
    ("V", 50),
    ("hytale-official", 10),
];

/// Get the priority for a provider by name
pub fn get_provider_priority(name: &str) -> i32 {
    PROVIDER_PRIORITIES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, p)| *p)
        .unwrap_or(0)
}