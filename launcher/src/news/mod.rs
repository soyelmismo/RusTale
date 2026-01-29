pub mod news;

use anyhow::{Context, Result};

pub use self::news::BlogPost;

pub async fn fetch_news(client: &reqwest::Client) -> Result<Vec<BlogPost>> {
    let response = client
        .get("https://hytale.com/api/blog/post/published")
        .send()
        .await
        .context("Failed to fetch news")?;

    if !response.status().is_success() {
        anyhow::bail!("News API error: HTTP {}", response.status());
    }

    let posts: Vec<BlogPost> = response.json().await.context("Failed to parse news JSON")?;

    // Limitar a las 10 noticias mas recientes (aumentado de 5)
    let mut limited_posts = posts.into_iter().take(10).collect::<Vec<_>>();

    // Ordenar por fecha (mas reciente primero)
    limited_posts.sort_by(|a, b| b.published_at.cmp(&a.published_at));

    Ok(limited_posts)
}
