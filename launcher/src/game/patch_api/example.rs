use std::sync::Arc;
use anyhow::Result;

use super::{
    PatchApiManager,
    EstrogenProvider,
    HytaleProvider,
    ShipOfYarnProvider,
};

/// Example of how to use the patch API system
pub async fn example_usage() -> Result<()> {
    // Create a new patch API manager - providers are now automatically included
    let manager = PatchApiManager::new();

    // Get current platform info
    let os = std::env::consts::OS;
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    };
    let channel = "release";

    println!("Checking for updates on {}-{} ({})", os, arch, channel);

    // Try to get the latest version from any provider
    match manager.get_latest_version(channel, os, arch).await {
        Ok(latest_version) => {
            println!("Latest version available: {}", latest_version);
            
            // Get all available versions
            match manager.get_available_versions(channel, os, arch).await {
                Ok(versions) => {
                    println!("Available versions: {:?}", versions);
                    
                    // Example: Get patch URL for version 0 -> latest
                    if latest_version > 0 {
                        match manager.get_patch_url(channel, os, arch, 0, latest_version).await {
                            Ok(patch_url) => {
                                println!("Complete patch URL: {}", patch_url);
                                
                                // Get signature URL
                                match manager.get_patch_signature_url(channel, os, arch, 0, latest_version).await {
                                    Ok(sig_url) => println!("Patch signature URL: {}", sig_url),
                                    Err(e) => println!("Warning: Could not get signature URL: {}", e),
                                }
                            }
                            Err(e) => println!("Error getting patch URL: {}", e),
                        }
                    }
                }
                Err(e) => println!("Error getting available versions: {}", e),
            }
        }
        Err(e) => println!("Error getting latest version: {}", e),
    }

    // Try to get JRE URL
    match manager.get_jre_url(os, arch).await {
        Ok(jre_url) => println!("JRE URL: {}", jre_url),
        Err(e) => println!("Error getting JRE URL: {}", e),
    }

    // Try to get Butler URL
    match manager.get_butler_url(os, arch).await {
        Ok(butler_url) => println!("Butler URL: {}", butler_url),
        Err(e) => println!("Error getting Butler URL: {}", e),
    }

    Ok(())
}

/// Example of using individual providers
pub async fn individual_provider_example() -> Result<()> {
    let os = std::env::consts::OS;
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    };
    let channel = "release";

    // Use Estrogen provider directly
    let estrogen_provider = EstrogenProvider::new();
    
    if estrogen_provider.is_available().await {
        println!("Estrogen provider is available");
        
        match estrogen_provider.get_latest_version(channel, os, arch).await {
            Ok(version) => println!("Latest version from Estrogen: {}", version),
            Err(e) => println!("Error from Estrogen: {}", e),
        }
    } else {
        println!("Estrogen provider is not available");
    }

    // Use fallback provider directly
    let shipofyarn_provider = ShipOfYarnProvider::new();
    
    if shipofyarn_provider.is_available().await {
        println!("ShipOfYarn provider is available");
        
        match shipofyarn_provider.get_latest_version(channel, os, arch).await {
            Ok(version) => println!("Latest version from ShipOfYarn: {}", version),
            Err(e) => println!("Error from ShipOfYarn: {}", e),
        }
    } else {
        println!("ShipOfYarn provider is not available");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_manager_creation() {
        let manager = PatchApiManager::new();
        // Should automatically include EstrogenProvider and ShipOfYarnProvider
        assert_eq!(manager.providers.len(), 2);
    }

    #[tokio::test]
    async fn test_custom_providers() {
        let mut manager = PatchApiManager::new();
        
        // Clear default providers and add custom ones
        manager.providers.clear();
        let provider = Arc::new(ShipOfYarnProvider::new());
        manager.providers.push(provider);
        
        assert_eq!(manager.providers.len(), 1);
    }

    #[tokio::test]
    async fn test_provider_availability() {
        let shipofyarn = ShipOfYarnProvider::new();
        let estrogen = EstrogenProvider::new();
        
        // These should not panic
        let _shipofyarn_available = shipofyarn.is_available().await;
        let _estrogen_available = estrogen.is_available().await;
    }
}
