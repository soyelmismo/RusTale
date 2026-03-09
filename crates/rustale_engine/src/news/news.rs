use chrono::{DateTime, Datelike, Utc};
use serde::{Deserialize, Serialize};
use feed_rs::model::Entry;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverImage {
    #[serde(rename = "s3Key")]
    pub s3_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlogPost {
    #[serde(rename = "_id")]
    pub id: String,

    pub title: String,

    #[serde(rename = "publishedAt")]
    pub published_at: DateTime<Utc>,

    pub slug: String,

    // URL completa del post (del RSS)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    // Hacemos cover_image opcional porque algunos posts pueden no tener imagen
    #[serde(rename = "coverImage")]
    pub cover_image: Option<CoverImage>,

    // Hacemos body_excerpt opcional y con default para evitar errores si es null
    #[serde(rename = "bodyExcerpt", default)]
    pub body_excerpt: Option<String>,

    pub author: String,

    // Usamos default para que si el campo falta, sea false/vacio
    #[serde(default)]
    pub featured: bool,

    #[serde(default)]
    pub tags: Vec<String>,

    #[serde(rename = "createdAt")]
    pub created_at: DateTime<Utc>,
}

impl BlogPost {
    // Crear desde una entrada RSS
    pub fn from_rss_entry(entry: &Entry) -> Self {
        let post_url = entry.links.first().map(|l| l.href.clone()).unwrap_or_default();
        
        Self {
            id: entry.id.clone(),
            title: entry.title.as_ref().map(|t| t.content.as_str()).unwrap_or("Untitled").to_string(),
            published_at: entry.published.unwrap_or_else(|| chrono::Utc::now()),
            slug: extract_slug_from_url(&post_url).unwrap_or_else(|| "unknown".to_string()),
            url: Some(post_url),
            cover_image: None, // Será cargado asíncronamente
            body_excerpt: entry.summary.as_ref().map(|s| s.content.as_str()).map(|s| {
                // Limitar excerpt y limpiar HTML
                let clean = strip_html_tags(s);
                if clean.len() > 200 {
                    format!("{}...", &clean[..200])
                } else {
                    clean
                }
            }),
            author: entry.authors.first().map(|a| a.name.as_str()).unwrap_or("Hytale Team").to_string(),
            featured: false, // RSS no tiene campo featured
            tags: vec![], // RSS puede tener categorías pero las ignoramos por ahora
            created_at: entry.published.unwrap_or_else(|| chrono::Utc::now()),
        }
    }
    // Ahora devuelve Option<String> porque puede no haber imagen
    // El s3_key ya incluye el prefijo completo (blog_cover_... o blog_thumb_...)
    pub fn get_image_url(&self) -> Option<String> {
        self.cover_image
            .as_ref()
            .map(|img| format!("https://cdn.hytale.com/variants/{}", img.s3_key))
    }
    
    pub fn has_image(&self) -> bool {
        self.cover_image.is_some()
    }
    
    pub fn get_image_key(&self) -> Option<&str> {
        self.cover_image.as_ref().map(|img| img.s3_key.as_str())
    }

    pub fn get_post_url(&self) -> String {
        // Usar la URL del RSS si está disponible, sino construir una
        if let Some(ref url) = self.url {
            url.clone()
        } else {
            let date = self.published_at.date_naive();
            let year = date.year();
            let month = format!("{:02}", date.month());
            format!("https://hytale.com/news/{}/{}/{}", year, month, self.slug)
        }
    }

    pub fn format_date(&self) -> String {
        self.published_at.format("%b %d, %Y").to_string()
    }
    
    pub fn is_featured(&self) -> bool {
        self.featured
    }
    
    pub fn get_tags_display(&self) -> String {
        if self.tags.is_empty() {
            // TODO: Pass localization context here
            "No tags".to_string() // Temporal hasta que se pueda pasar localización
        } else {
            self.tags.join(", ")
        }
    }
}
// Función asíncrona para extraer imágenes de una página de noticia
pub async fn extract_image_from_page(url: &str) -> Option<CoverImage> {
    if !url.starts_with("https://hytale.com/news/") {
        return None;
    }
    
    // Intentar obtener la página
    let response = rustale_shared::HTTP_CLIENT
        .get(url)
        .send()
        .await
        .ok()?;
    
    if !response.status().is_success() {
        return None;
    }
    
    let html = response.text().await.ok()?;
    
    // Buscar imágenes con el patrón de blog_cover (imagen de portada principal)
    // Priorizar blog_cover sobre blog_thumb
    let cover_regex = regex::Regex::new(r#"cdn\.hytale\.com/variants/(blog_cover_[^"']+)"#).ok()?;
    
    if let Some(captures) = cover_regex.captures(&html) {
        if let Some(key_match) = captures.get(1) {
            // Mantener la extensión del archivo, es requerida por la CDN
            return Some(CoverImage {
                s3_key: key_match.as_str().to_string(),
            });
        }
    }
    
    // Si no hay blog_cover, buscar blog_thumb como fallback
    let thumb_regex = regex::Regex::new(r#"cdn\.hytale\.com/variants/(blog_thumb_[^"']+)"#).ok()?;
    
    if let Some(captures) = thumb_regex.captures(&html) {
        if let Some(key_match) = captures.get(1) {
            // Mantener la extensión del archivo, es requerida por la CDN
            return Some(CoverImage {
                s3_key: key_match.as_str().to_string(),
            });
        }
    }
    
    None
}

// Función auxiliar para extraer slug de URL
fn extract_slug_from_url(url: &str) -> Option<String> {
    if url.is_empty() {
        return None;
    }
    
    // URL pattern: https://hytale.com/news/2019/2/some-post-title/
    let parts: Vec<&str> = url.split('/').collect();
    if parts.len() >= 6 && parts[0] == "https:" && parts[2] == "hytale.com" && parts[3] == "news" {
        Some(parts[5].to_string())
    } else {
        None
    }
}

// Función simple para limpiar tags HTML
fn strip_html_tags(html: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    
    // Limpiar entidades HTML comunes
    result.replace("&apos;", "'")
           .replace("&amp;", "&")
           .replace("&quot;", "\"")
           .replace("&lt;", "<")
           .replace("&gt;", ">")
           .trim()
           .to_string()
}
