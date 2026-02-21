use rust_embed::RustEmbed;
pub use serde_json::Value;

#[derive(RustEmbed)]
#[folder = "locales"]
struct Asset;

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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
    current_data: Value,                    
    fallback_data: Value,                   
    pub available_languages: Vec<Language>, 
}

impl Localization {
    pub fn new() -> Self {
        let fallback_json = Self::parse_embedded_file("en-US.json")
            .expect("CRITICAL: en-US.json missing inside binary!");

        Self {
            current_data: fallback_json.clone(), 
            fallback_data: fallback_json,
            available_languages: Vec::new(),
        }
    }

    pub fn load_available_languages(&mut self) {
        let mut langs = Vec::new();
        for file in Asset::iter() {
            let filename: &str = file.as_ref();
            if filename.ends_with(".json") {
                let id = filename.trim_end_matches(".json").to_string();
                if let Some(json) = Self::parse_embedded_file(filename) {
                    let name = json
                        .get("_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&id)
                        .to_string();
                    langs.push(Language { id, name });
                }
            }
        }
        langs.sort_by(|a, b| a.name.cmp(&b.name));
        self.available_languages = langs;
    }

    pub fn load_language(&mut self, lang_id: &str) {
        let filename = format!("{}.json", lang_id);
        if let Some(json) = Self::parse_embedded_file(&filename) {
            self.current_data = json;
        } else {
            self.current_data = self.fallback_data.clone();
        }
    }

    pub fn t<'a>(&'a self, key: &'a str) -> &'a str {
        if let Some(val) = Self::find_value(&self.current_data, key) {
            return val;
        }
        if let Some(val) = Self::find_value(&self.fallback_data, key) {
            return val;
        }
        key
    }

    pub fn ta<'a>(&'a self, key: &'a str, args: &[&str]) -> String {
        let mut text = self.t(key).to_string();
        for (i, arg) in args.iter().enumerate() {
            let placeholder = format!("{{{}}}", i);
            text = text.replace(&placeholder, arg);
        }
        text
    }

    fn parse_embedded_file(filename: &str) -> Option<Value> {
        let f = Asset::get(filename)?;
        serde_json::from_slice(&f.data).ok()
    }

    fn find_value<'a>(json: &'a Value, path: &str) -> Option<&'a str> {
        let parts: Vec<&str> = path.split('.').collect();
        let mut current = json;
        for part in parts {
            current = current.get(part)?;
        }
        current.as_str()
    }
}
