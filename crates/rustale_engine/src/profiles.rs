use rustale_shared::profiles::{Profile, ProfileStorage, ProfilesConfig};
use std::path::PathBuf;

pub struct ProfileService {
    storage: Box<dyn ProfileStorage>,
}

impl ProfileService {
    pub fn new(storage: Box<dyn ProfileStorage>) -> Self {
        Self { storage }
    }

    /// Ensures a valid state exists. Returns the active profile.
    pub fn ensure_initialized(&self) -> anyhow::Result<Profile> {
        let mut config = self.storage.load().unwrap_or_default();
        
        if config.profiles.is_empty() {
             // Logic to create default profile if none exists
             // (Though Default impl of ProfilesConfig already does this, 
             //  this handles the case where the file exists but has empty list which shouldn't happen)
             let default_profile = Profile {
                id: uuid::Uuid::new_v4(),
                name: "Player".to_string(),
            };
            config.profiles.push(default_profile.clone());
            config.current_profile = default_profile.id;
            self.storage.save(&config)?;
            return Ok(default_profile);
        }
        
        // Return active profile or first
        Ok(config.get_active_profile().unwrap_or_else(|| config.profiles[0].clone()))
    }
}

pub struct FileProfileStorage {
    path: PathBuf,
}

impl FileProfileStorage {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl ProfileStorage for FileProfileStorage {
    fn load(&self) -> anyhow::Result<ProfilesConfig> {
        if !self.path.exists() {
             return Ok(ProfilesConfig::default());
        }
        let content = std::fs::read_to_string(&self.path)?;
        Ok(toml::from_str(&content).unwrap_or_default())
    }

    fn save(&self, config: &ProfilesConfig) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Atomic save: write to tmp then rename
        let tmp_path = self.path.with_extension("tmp");
        let content = toml::to_string_pretty(config)?;
        std::fs::write(&tmp_path, content)?;
        std::fs::rename(&tmp_path, &self.path)?;
        Ok(())
    }
}
