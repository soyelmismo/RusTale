use rust_embed::RustEmbed;
use serde_json::Value;

#[derive(RustEmbed)]
#[folder = "locales"]
struct Asset;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Language {
    pub id: String,   // Ej: "es-ES"
    pub name: String, // Ej: "English"
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

#[derive(Debug, Clone)]
pub struct Localization {
    current_data: Value,                    // RAM: Only active language
    fallback_data: Value,                   // RAM: Always English (backup)
    pub available_languages: Vec<Language>, // Light list (only names)
}

impl Localization {
    pub fn new() -> Self {
        let fallback_json = Self::parse_embedded_file("en-US.json")
            .expect("CRITICAL: en-US.json missing inside binary!");

        Self {
            current_data: fallback_json.clone(), // Empezamos en inglés
            fallback_data: fallback_json,
            available_languages: Vec::new(),
        }
    }

    /// Scans internal files.
    /// RAM OPTIMIZATION: Opens the JSON, reads the name and closes it.
    pub fn load_available_languages(&mut self) {
        let mut langs = Vec::new();

        println!("[Lang] Scanning embedded locales...");
        for file in Asset::iter() {
            let filename: &str = file.as_ref();
            println!("[Lang] Found embedded file: {}", filename);

            if filename.ends_with(".json") {
                // Extract ID safely (e.g: "es-ES" from "es-ES.json")
                let id = filename.trim_end_matches(".json").to_string();

                if let Some(json) = Self::parse_embedded_file(filename) {
                    let name = json
                        .get("_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&id)
                        .to_string();

                    println!("[Lang] Registered language: {} ({})", name, id);
                    langs.push(Language { id, name });
                } else {
                    println!("[Lang] ERROR: Could not parse JSON for {}", filename);
                }
            }
        }

        langs.sort_by(|a, b| a.name.cmp(&b.name));
        self.available_languages = langs;
    }

    /// Loads a full language into RAM
    pub fn load_language(&mut self, lang_id: &str) {
        let filename = format!("{}.json", lang_id);
        println!("[Lang] Loading language file: {}", filename);

        if let Some(json) = Self::parse_embedded_file(&filename) {
            self.current_data = json;
            println!("[Lang] Language '{}' loaded successfully", lang_id);
        } else {
            println!(
                "[Lang] ERROR: Failed to load language {}, reverting to fallback",
                lang_id
            );
            self.current_data = self.fallback_data.clone();
        }
    }

    /// Searches for translation with Triple Fallback
    pub fn t<'a>(&'a self, key: &'a str) -> &'a str {
        // 1. Try current language
        if let Some(val) = Self::find_value(&self.current_data, key) {
            return val;
        }
        // 2. Try fallback (English)
        if let Some(val) = Self::find_value(&self.fallback_data, key) {
            return val;
        }
        // 3. Returns the key if not found
        key
    }

    /// Searches for translation with arguments (format: {0}, {1}, ...)
    pub fn ta<'a>(&'a self, key: &'a str, args: &[&str]) -> String {
        let mut text = self.t(key).to_string();
        for (i, arg) in args.iter().enumerate() {
            let placeholder = format!("{{{}}}", i);
            text = text.replace(&placeholder, arg);
        }
        text
    }

    /// Helper to read from binary
    fn parse_embedded_file(filename: &str) -> Option<Value> {
        let f = Asset::get(filename)?;
        // from_slice is very efficient
        serde_json::from_slice(&f.data).ok()
    }

    /// Helper to navigate the JSON (e.g: "launcher.buttons.play")
    fn find_value<'a>(json: &'a Value, path: &str) -> Option<&'a str> {
        let parts: Vec<&str> = path.split('.').collect();
        let mut current = json;
        for part in parts {
            current = current.get(part)?;
        }
        current.as_str()
    }
}
