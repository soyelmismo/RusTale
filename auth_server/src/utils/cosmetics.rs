use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;

use crate::models::{CosmeticDefinition, CosmeticItemResponse};

pub fn read_cosmetic_inventory_from_zip(zip_path: &PathBuf) -> String {
    if !zip_path.exists() {
        return "{}".to_string();
    }

    let file = match std::fs::File::open(zip_path) {
        Ok(f) => f,
        Err(_) => return "{}".to_string(),
    };

    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(_) => return "{}".to_string(),
    };

    let mut inventory: HashMap<String, Vec<String>> = HashMap::new();
    let categories = vec![
        "BodyCharacteristics", "Capes", "EarAccessory", "Ears", "Eyebrows", "Eyes",
        "Faces", "FaceAccessory", "FacialHair", "Gloves", "Haircuts", "HeadAccessory",
        "Mouths", "Overpants", "Overtops", "Pants", "Shoes", "SkinFeatures",
        "Undertops", "Underwear",
    ];

    for category in categories {
        let inner_path = format!("Cosmetics/CharacterCreator/{}.json", category);
        let field_name = get_exact_field_name(category);

        if let Ok(mut f) = archive.by_name(&inner_path) {
            let mut content = String::new();
            if f.read_to_string(&mut content).is_ok() {
                if let Ok(items) = serde_json::from_str::<Vec<CosmeticDefinition>>(&content) {
                    let ids: Vec<String> = items.into_iter().map(|i| i.id).collect();
                    inventory.insert(field_name.to_string(), ids);
                }
            }
        }
    }

    serde_json::to_string(&inventory).unwrap_or("{}".to_string())
}

pub fn read_cosmetics_from_zip(zip_path: &PathBuf) -> String {
    println!("Loading structured cosmetics from Assets.zip...");
    if !zip_path.exists() {
        return "{}".to_string();
    }

    let file = match std::fs::File::open(zip_path) {
        Ok(f) => f,
        Err(_) => return "{}".to_string(),
    };

    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(_) => return "{}".to_string(),
    };

    // 1. Load GradientSets
    let mut gradient_data = HashMap::new();
    if let Ok(mut g_file) = archive.by_name("Cosmetics/CharacterCreator/GradientSets.json") {
        let mut content = String::new();
        if g_file.read_to_string(&mut content).is_ok() {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(sets) = val.as_array() {
                    for set in sets {
                        if let (Some(id), Some(gradients)) = (set.get("Id"), set.get("Gradients")) {
                            if let (Some(id_str), Some(grad_obj)) = (id.as_str(), gradients.as_object()) {
                                let color_ids: Vec<String> = grad_obj.keys().cloned().collect();
                                gradient_data.insert(id_str.to_string(), color_ids);
                            }
                        }
                    }
                }
            }
        }
    }

    let mut inventory: HashMap<String, Vec<CosmeticItemResponse>> = HashMap::new();

    let categories = vec![
        "BodyCharacteristics", "Capes", "EarAccessory", "Ears", "Eyebrows", "Eyes",
        "Faces", "FaceAccessory", "FacialHair", "Gloves", "Haircuts", "HeadAccessory",
        "Mouths", "Overpants", "Overtops", "Pants", "Shoes", "SkinFeatures",
        "Undertops", "Underwear",
    ];

    for category in categories {
        let inner_path = format!("Cosmetics/CharacterCreator/{}.json", category);
        let field_name = get_exact_field_name(category);

        if let Ok(mut f) = archive.by_name(&inner_path) {
            let mut content = String::new();
            if f.read_to_string(&mut content).is_ok() {
                if let Ok(items) = serde_json::from_str::<Vec<CosmeticDefinition>>(&content) {
                    let mut transformed = Vec::new();
                    for item in items {
                        // Skip incomplete or hidden items
                        if item.name.is_none() && item.model.is_none() && item.variants.is_none() {
                            continue;
                        }

                        // Determine colors
                        let mut colors = Vec::new();
                        if let Some(ref gs_id) = item.gradient_set {
                            if let Some(c) = gradient_data.get(gs_id) {
                                colors = c.clone();
                            }
                        } else if let Some(ref tex) = item.textures {
                            if let Some(obj) = tex.as_object() {
                                colors = obj.keys().cloned().collect();
                            }
                        }

                        // Fallback thumbnail
                        let thumbnail = item.greyscale_texture.clone();

                        transformed.push(CosmeticItemResponse {
                            id: item.id.clone(),
                            name: item.name.unwrap_or_else(|| item.id.clone()),
                            thumbnail,
                            colors,
                            gradient_set: item.gradient_set,
                            model: item.model,
                            variants: None, // Simplified
                            head_accessory_type: item.head_accessory_type,
                            hair_type: item.hair_type,
                            requires_generic_haircut: item.requires_generic_haircut,
                        });
                    }
                    inventory.insert(field_name.to_string(), transformed);
                }
            }
        }
    }

    serde_json::to_string(&inventory).unwrap_or("{}".to_string())
}

fn get_exact_field_name(cat: &str) -> &'static str {
    match cat {
        "BodyCharacteristics" => "bodyCharacteristic",
        "Capes" => "cape",
        "Faces" => "face",
        "Haircuts" => "haircut",
        "Mouths" => "mouth",
        "Overtops" => "overtop",
        "Undertops" => "undertop",
        "SkinFeatures" => "skinFeature",
        "EarAccessory" => "earAccessory",
        "Ears" => "ears",
        "Eyebrows" => "eyebrows",
        "Eyes" => "eyes",
        "FaceAccessory" => "faceAccessory",
        "FacialHair" => "facialHair",
        "Gloves" => "gloves",
        "HeadAccessory" => "headAccessory",
        "Overpants" => "overpants",
        "Pants" => "pants",
        "Shoes" => "shoes",
        "Underwear" => "underwear",
        _ => "",
    }
}
