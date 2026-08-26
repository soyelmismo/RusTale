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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;
    use zip::write::SimpleFileOptions;

    #[test]
    fn test_nonexistent_zip_file_returns_empty_json() {
        let non_existent_path = PathBuf::from("non_existent_archive_12345.zip");
        let inventory_res = read_cosmetic_inventory_from_zip(&non_existent_path);
        assert_eq!(inventory_res, "{}");

        let cosmetics_res = read_cosmetics_from_zip(&non_existent_path);
        assert_eq!(cosmetics_res, "{}");
    }

    #[test]
    fn test_corrupted_zip_file_returns_empty_json() {
        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
        writeln!(temp_file, "This is not a zip file").expect("Failed to write to temp file");

        let zip_path = temp_file.path().to_path_buf();

        let inventory_res = read_cosmetic_inventory_from_zip(&zip_path);
        assert_eq!(inventory_res, "{}");

        let cosmetics_res = read_cosmetics_from_zip(&zip_path);
        assert_eq!(cosmetics_res, "{}");
    }

    #[test]
    fn test_read_cosmetic_inventory_from_valid_zip() {
        let temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let zip_path = temp_file.path().to_path_buf();

        let file = std::fs::File::create(&zip_path).expect("Failed to open file for zip creation");
        let mut zip_builder = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

        let haircut_json = r#"[
            {"Id": "haircut_short_01", "Name": "Short Hair"},
            {"Id": "haircut_long_01", "Name": "Long Hair"}
        ]"#;
        zip_builder
            .start_file("Cosmetics/CharacterCreator/Haircuts.json", options)
            .expect("Failed to start Haircuts file");
        zip_builder
            .write_all(haircut_json.as_bytes())
            .expect("Failed to write Haircuts file");

        let capes_json = r#"[
            {"Id": "cape_red", "Name": "Red Cape"}
        ]"#;
        zip_builder
            .start_file("Cosmetics/CharacterCreator/Capes.json", options)
            .expect("Failed to start Capes file");
        zip_builder
            .write_all(capes_json.as_bytes())
            .expect("Failed to write Capes file");

        zip_builder.finish().expect("Failed to finalize zip archive");

        let inventory_json = read_cosmetic_inventory_from_zip(&zip_path);
        let parsed: HashMap<String, Vec<String>> =
            serde_json::from_str(&inventory_json).expect("Failed to parse output JSON");

        assert_eq!(parsed.len(), 2);
        assert_eq!(
            parsed.get("haircut"),
            Some(&vec!["haircut_short_01".to_string(), "haircut_long_01".to_string()])
        );
        assert_eq!(parsed.get("cape"), Some(&vec!["cape_red".to_string()]));
    }

    #[test]
    fn test_read_cosmetic_inventory_malformed_json_in_zip() {
        let temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let zip_path = temp_file.path().to_path_buf();

        let file = std::fs::File::create(&zip_path).expect("Failed to open file for zip creation");
        let mut zip_builder = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

        let malformed_json = r#"[{"Id": "haircut_short_01", "Name": "Short Hair""#;
        zip_builder
            .start_file("Cosmetics/CharacterCreator/Haircuts.json", options)
            .expect("Failed to start Haircuts file");
        zip_builder
            .write_all(malformed_json.as_bytes())
            .expect("Failed to write malformed file");

        zip_builder.finish().expect("Failed to finalize zip archive");

        let inventory_json = read_cosmetic_inventory_from_zip(&zip_path);
        assert_eq!(inventory_json, "{}");
    }

    #[test]
    fn test_read_cosmetics_from_zip_with_gradient_sets_and_textures() {
        let temp_file = NamedTempFile::new().expect("Failed to create temp file");
        let zip_path = temp_file.path().to_path_buf();

        let file = std::fs::File::create(&zip_path).expect("Failed to open file for zip creation");
        let mut zip_builder = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

        // 1. GradientSets.json
        let gradient_json = r#"[
            {
                "Id": "hair_gradients",
                "Gradients": {
                    "black": {},
                    "brown": {},
                    "blonde": {}
                }
            }
        ]"#;
        zip_builder
            .start_file("Cosmetics/CharacterCreator/GradientSets.json", options)
            .expect("Failed to start GradientSets file");
        zip_builder
            .write_all(gradient_json.as_bytes())
            .expect("Failed to write GradientSets file");

        // 2. Haircuts.json with GradientSet reference & texture item & incomplete item
        let haircuts_json = r#"[
            {
                "Id": "hair_01",
                "Name": "Curly Hair",
                "GradientSet": "hair_gradients",
                "GreyscaleTexture": "hair_01_thumb.png"
            },
            {
                "Id": "hair_02",
                "Name": "Straight Hair",
                "Textures": {
                    "red": "tex_red.png",
                    "blue": "tex_blue.png"
                }
            },
            {
                "Id": "hidden_item"
            }
        ]"#;
        zip_builder
            .start_file("Cosmetics/CharacterCreator/Haircuts.json", options)
            .expect("Failed to start Haircuts file");
        zip_builder
            .write_all(haircuts_json.as_bytes())
            .expect("Failed to write Haircuts file");

        zip_builder.finish().expect("Failed to finalize zip archive");

        let cosmetics_json = read_cosmetics_from_zip(&zip_path);
        let parsed: HashMap<String, Vec<serde_json::Value>> =
            serde_json::from_str(&cosmetics_json).expect("Failed to parse cosmetics JSON");

        assert!(parsed.contains_key("haircut"));
        let haircut_items = parsed.get("haircut").unwrap();

        // hidden_item should be skipped (missing name, model, and variants)
        assert_eq!(haircut_items.len(), 2);

        let item1 = &haircut_items[0];
        assert_eq!(item1["id"], "hair_01");
        assert_eq!(item1["name"], "Curly Hair");
        assert_eq!(item1["thumbnail"], "hair_01_thumb.png");
        let colors1: Vec<String> = serde_json::from_value(item1["colors"].clone()).unwrap();
        assert_eq!(colors1.len(), 3);
        assert!(colors1.contains(&"black".to_string()));
        assert!(colors1.contains(&"brown".to_string()));
        assert!(colors1.contains(&"blonde".to_string()));

        let item2 = &haircut_items[1];
        assert_eq!(item2["id"], "hair_02");
        let colors2: Vec<String> = serde_json::from_value(item2["colors"].clone()).unwrap();
        assert_eq!(colors2.len(), 2);
        assert!(colors2.contains(&"red".to_string()));
        assert!(colors2.contains(&"blue".to_string()));
    }
}
