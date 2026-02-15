use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PlayerSkin {
    pub id: String,
    pub name: String,
    #[serde(rename = "skinData")]
    pub skin_data: String,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime<Utc>,
    #[serde(rename = "updatedAt", skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct UserSkinsData {
    #[serde(rename = "playerSkins")]
    pub player_skins: Vec<PlayerSkin>,
    #[serde(rename = "activeSkin")]
    pub active_skin: Option<String>,
    #[serde(default)]
    pub skin: serde_json::Value,
}

#[derive(Serialize)]
pub struct PlayerSkinsResponse {
    #[serde(rename = "activeSkin")]
    pub active_skin: Option<String>,
    #[serde(rename = "maxSkins")]
    pub max_skins: i32,
    pub skins: Vec<PlayerSkin>,
}

#[derive(Deserialize)]
pub struct PlayerSkinsPostRequest {
    pub name: Option<String>,
    #[serde(rename = "skinData")]
    pub skin_data: Option<String>,
}

#[derive(Deserialize)]
pub struct PlayerSkinsSetActiveRequest {
    #[serde(rename = "skinId")]
    pub skin_id: String,
}

// Default skin JSON (Fallback)
pub const DEFAULT_SKIN: &str = r#"{"bodyCharacteristic":"Muscular.09","underwear":"Boxer.Purple","face":"Face_Neutral","ears":"Default","mouth":"Mouth_Long","haircut":"SuperSlickback.PitchBlack","facialHair":null,"eyebrows":"Thin.PitchBlack","eyes":"Large_Eyes.GreenLight","pants":"Bermuda_Rolled.GreyBlue","overpants":null,"undertop":null,"overtop":"Winter_Jacket.Red","shoes":"BasicShoes_Sandals.Black","headAccessory":"StrawHat.Red","faceAccessory":"Plaster.Brown","earAccessory":null,"skinFeature":null,"gloves":null,"cape":null}"#;
