# Patch API System

This module provides a generic interface for managing multiple patch providers/mirrors for the Hytale launcher. It allows the launcher to automatically fall back to different APIs if one is unavailable.

## Architecture

The system consists of:

1. **PatchProvider Trait** - Generic interface that all providers must implement
2. **PatchApiManager** - Manager that handles multiple providers and provides fallback logic
3. **Provider Implementations** - Specific implementations for different APIs:
   - `HytaleProvider` - Official Hytale API (requires authentication)
   - `EstrogenProvider` - Estrogen mirror API (no auth required)
   - `ShipOfYarnProvider` - ShipOfYarn fallback API (no auth required)

## Usage

### Basic Usage

```rust
use crate::game::patch_api::PatchApiManager;

// Create manager - providers are automatically included
let manager = PatchApiManager::new();

// Get latest version (will try providers in order)
let latest = manager.get_latest_version("release", "windows", "amd64").await?;
```

### Provider Priority

Providers are tried in the order they are automatically included in the manager. The first provider that successfully returns a result is used.

Default priority order:
1. `EstrogenProvider` - Fast mirror
2. `ShipOfYarnProvider` - Fallback API

Note: `HytaleProvider` (official API) requires authentication and can be added manually if needed.

### Individual Provider Usage

You can also use providers directly:

```rust
use crate::game::patch_api::EstrogenProvider;

let provider = EstrogenProvider::new();
if provider.is_available().await {
    let latest = provider.get_latest_version("release", "windows", "amd64").await?;
    println!("Latest version: {}", latest);
}
```

## Provider Capabilities

### HytaleProvider
- ✅ Latest version
- ✅ Available versions
- ✅ Patch URLs
- ✅ Patch signatures
- ✅ Complete version check
- ✅ JRE URLs
- ❌ Butler URLs
- 🔐 Requires authentication

### EstrogenProvider
- ✅ Latest version
- ✅ Available versions
- ✅ Patch URLs
- ✅ Patch signatures
- ✅ Complete version check
- ✅ JRE URLs
- ❌ Butler URLs
- 🔓 No authentication required

### ShipOfYarnProvider
- ✅ Latest version
- ✅ Available versions
- ✅ Patch URLs
- ⚠️ Patch signatures (constructed, not provided)
- ✅ Complete version check
- ✅ JRE URLs
- ✅ Butler URLs
- 🔓 No authentication required

## Error Handling

The system uses `anyhow::Result` for error handling. If all providers fail, the manager returns an error indicating that no provider could fulfill the request.

## Thread Safety

All providers are `Send + Sync` and can be safely used across multiple threads. The manager uses `Arc<dyn PatchProvider>` to enable shared ownership.

## Caching

Some providers (like FallbackProvider) implement basic caching to avoid repeated API calls. This is transparent to the user.

## Testing

Run the example to test the system:

```rust
use crate::game::patch_api::example::example_usage;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    example_usage().await
}
```

## Migration from Legacy Code

To migrate from the old fallback system:

1. Replace `fallback::get_latest_version()` calls with `PatchApiManager::get_latest_version()`
2. Replace `fallback::get_version_url()` calls with `PatchApiManager::get_patch_url()`
3. Use `ShipOfYarnProvider` directly instead of the old fallback module

## Adding New Providers

To add a new provider:

1. Create a new module implementing the `PatchProvider` trait
2. Add the module to `mod.rs`
3. Export the provider in `mod.rs`
4. Add it to the `PatchApiManager::new()` method

The trait requires implementing:
- `name()` - Provider identifier
- `is_available()` - Check if provider is reachable
- `get_latest_version()` - Get latest version for channel
- `get_available_versions()` - Get all available versions
- `get_patch_url()` - Get patch download URL
- `get_patch_signature_url()` - Get patch signature URL
- `has_complete_version()` - Check if complete version exists