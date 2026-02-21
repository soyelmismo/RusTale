use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub struct Profile {
    pub id: Uuid,
    pub name: String,
}

impl std::fmt::Display for Profile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct ProfilesConfig {
    pub profiles: Vec<Profile>,
    #[serde(rename = "current_profile")]
    pub current_profile: Uuid,
}

impl Default for ProfilesConfig {
    fn default() -> Self {
        let id = Uuid::new_v4();
        Self {
            profiles: vec![Profile {
                id,
                name: "Player".to_string(),
            }],
            current_profile: id,
        }
    }
}

impl ProfilesConfig {
    pub fn get_active_profile(&self) -> Option<Profile> {
        self.profiles
            .iter()
            .find(|p| p.id == self.current_profile)
            .cloned()
    }

    pub fn get_current_profile_name(&self) -> String {
        self.get_active_profile()
            .map(|p| p.name)
            .unwrap_or_else(|| "Player".to_string())
    }
}

/// Trait to allow abstracting the storage mechanism for profiles
pub trait ProfileStorage: Send + Sync {
    fn load(&self) -> anyhow::Result<ProfilesConfig>;
    fn save(&self, config: &ProfilesConfig) -> anyhow::Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profiles_config_get_active_profile() {
        let config = ProfilesConfig::default();
        let active = config.get_active_profile();
        assert!(active.is_some());
        assert_eq!(active.unwrap().name, "Player");
    }

    #[test]
    fn test_profiles_config_get_active_profile_none() {
        let mut config = ProfilesConfig::default();
        // Set a non-existent profile ID
        config.current_profile = Uuid::new_v4();
        let active = config.get_active_profile();
        assert!(active.is_none());
    }

    #[test]
    fn test_profiles_config_get_current_profile_name_fallback() {
        let mut config = ProfilesConfig::default();
        // Set a non-existent profile ID
        config.current_profile = Uuid::new_v4();
        let name = config.get_current_profile_name();
        assert_eq!(name, "Player"); // Falls back to default
    }

    #[test]
    fn test_profiles_config_multiple_profiles() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let config = ProfilesConfig {
            profiles: vec![
                Profile { id: id1, name: "Player1".to_string() },
                Profile { id: id2, name: "Player2".to_string() },
            ],
            current_profile: id2,
        };
        
        let active = config.get_active_profile();
        assert!(active.is_some());
        assert_eq!(active.unwrap().name, "Player2");
    }
}
