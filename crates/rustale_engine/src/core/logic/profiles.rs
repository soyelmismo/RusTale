/// Profile management logic module
/// Centralizes all profile-related operations for the Core
use rustale_shared::profiles::{Profile, ProfilesConfig};
use std::path::PathBuf;
use anyhow::Result;

pub struct ProfileManager {
    pub profiles: ProfilesConfig,
}

impl ProfileManager {
    pub fn new(profiles: ProfilesConfig) -> Self {
        Self { profiles }
    }

    /// STRICT: Ensures at least one profile exists. Returns true if a new one was created.
    pub fn ensure_integrity(&mut self) -> bool {
        if self.profiles.profiles.is_empty() {
            println!("[ProfileManager] No profiles found. Creating default 'Player'.");
            let default_id = uuid::Uuid::new_v4();
            self.profiles.profiles.push(Profile {
                id: default_id,
                name: "Player".to_string(),
            });
            self.profiles.current_profile = default_id;
            return true;
        }
        // Validate current_profile points to existing profile
        if !self.profiles.profiles.iter().any(|p| p.id == self.profiles.current_profile) {
             if let Some(first) = self.profiles.profiles.first() {
                 println!("[ProfileManager] Active profile ID invalid. Resetting to first available.");
                 self.profiles.current_profile = first.id;
                 return true;
             }
        }
        false
    }

    /// Update profiles configuration
    pub fn update_profiles(&mut self, profiles: ProfilesConfig) {
        self.profiles = profiles;
    }

    /// Save profiles to disk
    pub async fn save_profiles(&self) -> Result<()> {
        crate::system::save_profiles(&self.profiles).await
    }

    /// Import profiles from a file
    pub async fn import_profiles(&mut self, path: &PathBuf) -> Result<usize> {
        let imported_content = tokio::fs::read_to_string(path).await?;
        let imported_config: ProfilesConfig = serde_json::from_str(&imported_content)?;
        
        let initial_count = self.profiles.profiles.len();
        
        // Add new profiles, avoiding duplicates by UUID
        for profile in imported_config.profiles {
            if !self.profiles.profiles.iter().any(|p| p.id == profile.id) {
                self.profiles.profiles.push(profile);
            }
        }
        
        Ok(self.profiles.profiles.len() - initial_count)
    }

    /// Import profiles from a vector (already loaded)
    pub fn import_profiles_from_vec(&mut self, profiles: &[Profile]) -> Result<usize> {
        let initial_count = self.profiles.profiles.len();
        
        // Add new profiles, avoiding duplicates by UUID
        for profile in profiles {
            if !self.profiles.profiles.iter().any(|p| p.id == profile.id) {
                self.profiles.profiles.push(profile.clone());
            }
        }
        
        Ok(self.profiles.profiles.len() - initial_count)
    }

    /// Create a new profile
    pub fn create_profile(&mut self, name: String) -> Profile {
        let new_profile = Profile {
            id: uuid::Uuid::new_v4(),
            name,
        };
        self.profiles.profiles.push(new_profile.clone());
        new_profile
    }

    /// Update an existing profile
    pub fn update_profile(&mut self, id: uuid::Uuid, name: String) -> Option<Profile> {
        if let Some(profile) = self.profiles.profiles.iter_mut().find(|p| p.id == id) {
            profile.name = name;
            Some(profile.clone())
        } else {
            None
        }
    }

    /// Update profile UUID
    pub fn update_profile_uuid(&mut self, old_id: uuid::Uuid, new_id: uuid::Uuid) -> Option<Profile> {
        if let Some(profile) = self.profiles.profiles.iter_mut().find(|p| p.id == old_id) {
            profile.id = new_id;
            
            // Update current profile if it was the one we modified
            if self.profiles.current_profile == old_id {
                self.profiles.current_profile = new_id;
            }
            
            Some(profile.clone())
        } else {
            None
        }
    }

    /// Delete a profile
    pub fn delete_profile(&mut self, id: uuid::Uuid) -> bool {
        let initial_len = self.profiles.profiles.len();
        self.profiles.profiles.retain(|p| p.id != id);
        
        // If we deleted the current profile, switch to the first one
        if self.profiles.current_profile == id {
            if let Some(first) = self.profiles.profiles.first() {
                self.profiles.current_profile = first.id;
            }
        }
        
        self.profiles.profiles.len() < initial_len
    }

    /// Set current profile
    pub fn set_current_profile(&mut self, id: uuid::Uuid) -> bool {
        if self.profiles.profiles.iter().any(|p| p.id == id) {
            self.profiles.current_profile = id;
            true
        } else {
            false
        }
    }

    /// Get active profile
    pub fn get_active_profile(&self) -> Option<Profile> {
        self.profiles.get_active_profile()
    }

    /// Get a clone of the full profiles config
    pub fn get_config(&self) -> ProfilesConfig {
        self.profiles.clone()
    }
}

/// Load profile file from disk
pub async fn load_profile_file(path: &PathBuf) -> Result<ProfilesConfig, String> {
    match tokio::fs::read_to_string(path).await {
        Ok(content) => {
            match serde_json::from_str::<ProfilesConfig>(&content) {
                Ok(config) => Ok(config),
                Err(e) => Err(format!("Failed to parse profile file: {}", e)),
            }
        }
        Err(e) => Err(format!("Failed to read profile file: {}", e)),
    }
}
