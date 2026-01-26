use chrono::{DateTime, Datelike, Utc};
use serde::{Deserialize, Serialize};

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
    // Ahora devuelve Option<String> porque puede no haber imagen
    pub fn get_image_url(&self) -> Option<String> {
        self.cover_image
            .as_ref()
            .map(|img| format!("https://cdn.hytale.com/variants/blog_thumb_{}", img.s3_key))
    }

    pub fn get_post_url(&self) -> String {
        let date = self.published_at.date_naive();
        let year = date.year();
        let month = format!("{:02}", date.month());
        format!("https://hytale.com/news/{}/{}/{}", year, month, self.slug)
    }

    pub fn format_date(&self) -> String {
        self.published_at.format("%b %d, %Y").to_string()
    }
}
