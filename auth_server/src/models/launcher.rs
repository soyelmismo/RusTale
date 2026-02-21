use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LauncherData {
    #[serde(rename = "EulaAcceptedAt")]
    pub eula_accepted_at: DateTime<Utc>,
    #[serde(rename = "Owner")]
    pub owner: String,
    #[serde(rename = "Patchlines")]
    pub patchlines: Patchlines,
    #[serde(rename = "Profiles")]
    pub profiles: Vec<LauncherProfileInfo>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LauncherProfileInfo {
    #[serde(rename = "UUID")]
    pub uuid: String,
    #[serde(rename = "Username")]
    pub username: String,
    #[serde(rename = "Entitlements")]
    pub entitlements: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Patchlines {
    #[serde(rename = "PreRelease")]
    pub pre_release: GameVersionInfo,
    #[serde(rename = "Release")]
    pub release: GameVersionInfo,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GameVersionInfo {
    #[serde(rename = "BuildVersion")]
    pub build_version: String,
    #[serde(rename = "Newest")]
    pub newest: i32,
}

#[derive(Deserialize)]
pub struct UpdatePathRequest {
    pub game_dir: String,
}

#[derive(Deserialize)]
pub struct UpdateIdentityRequest {
    pub username: String,
    pub uuid: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AccountInfo {
    #[serde(rename = "createdAt", alias = "CreatedAt")]
    pub created_at: DateTime<Utc>,

    #[serde(alias = "Entitlements")]
    pub entitlements: Vec<String>,

    #[serde(rename = "nextNameChangeAt")]
    pub next_name_change_at: DateTime<Utc>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub skin: Option<String>,

    #[serde(alias = "Username")]
    pub username: String,

    #[serde(alias = "UUID")]
    pub uuid: String,
}

pub const ENTITLEMENTS: &[&str] = &["game.base", "game.deluxe", "game.founder", "game.server"];
