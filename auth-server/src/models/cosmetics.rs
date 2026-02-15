use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CosmeticDefinition {
    #[serde(rename = "Id")]
    pub id: String,
    #[serde(rename = "Name", default)]
    pub name: Option<String>,
    #[serde(rename = "Model", default)]
    pub model: Option<String>,
    #[serde(rename = "GreyscaleTexture", default)]
    pub greyscale_texture: Option<String>,
    #[serde(rename = "Textures", default)]
    pub textures: Option<serde_json::Value>,
    #[serde(rename = "GradientSet", default)]
    pub gradient_set: Option<String>,
    #[serde(rename = "Variants", default)]
    pub variants: Option<serde_json::Value>,
    #[serde(rename = "HeadAccessoryType", default)]
    pub head_accessory_type: Option<String>,
    #[serde(rename = "HairType", default)]
    pub hair_type: Option<String>,
    #[serde(rename = "RequiresGenericHaircut", default)]
    pub requires_generic_haircut: Option<bool>,
}

#[derive(Serialize)]
pub struct CosmeticItemResponse {
    pub id: String,
    pub name: String,
    pub thumbnail: Option<String>,
    pub colors: Vec<String>,
    #[serde(rename = "gradientSet")]
    pub gradient_set: Option<String>,
    pub model: Option<String>,
    pub variants: Option<Vec<CosmeticVariant>>,
    #[serde(rename = "headAccessoryType", skip_serializing_if = "Option::is_none")]
    pub head_accessory_type: Option<String>,
    #[serde(rename = "hairType", skip_serializing_if = "Option::is_none")]
    pub hair_type: Option<String>,
    #[serde(
        rename = "requiresGenericHaircut",
        skip_serializing_if = "Option::is_none"
    )]
    pub requires_generic_haircut: Option<bool>,
}

#[derive(Serialize)]
pub struct CosmeticVariant {
    pub id: String,
    pub name: String,
}
